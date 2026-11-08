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

//! Random sequences of everything the game can ask, with the values it
//! should not send among them: whatever the sequence, the mixer neither
//! panics nor sends the device anything but finite samples below full
//! scale, and once everything is stopped, every sound comes back.

use std::sync::Arc;

use nadir_audio::{AudioConfig, Bus, MusicData, PlayParams, SoundData, VoiceChange};
use nadir_codec::qoa;
use nadir_core::Pcg32;
use nadir_core::Vec2;
use nadir_core::testing::check;

#[derive(Debug)]
enum Action {
    Play {
        sound: usize,
        params: PlayParams,
        at: Option<Vec2>,
    },
    Stop {
        voice: usize,
        fade_ms: u32,
    },
    Set {
        voice: usize,
        change: VoiceChange,
    },
    BusVolume {
        bus: Bus,
        volume: f32,
        fade_ms: u32,
    },
    Listener(Vec2),
    Music {
        track: usize,
        crossfade_ms: u32,
    },
    StopMusic(u32),
    Render {
        frames: usize,
        channels: usize,
        rate: u32,
    },
    Update,
}

#[derive(Debug)]
struct Case {
    voices: usize,
    commands: usize,
    actions: Vec<Action>,
}

const FLOATS: [f32; 16] = [
    0.0,
    1.0,
    -1.0,
    0.5,
    16.0,
    1.37,
    0.0625,
    1e-30,
    1e30,
    -1e30,
    f32::MAX,
    -f32::MAX,
    f32::INFINITY,
    f32::NEG_INFINITY,
    f32::NAN,
    -0.0,
];

fn float(rng: &mut Pcg32) -> f32 {
    if rng.range(0..3u32) == 0 {
        FLOATS[rng.range(0..FLOATS.len() as u32) as usize]
    } else {
        rng.range(-2.0..2.0)
    }
}

fn position(rng: &mut Pcg32) -> Vec2 {
    let mut coordinate = || {
        if rng.range(0..4u32) == 0 {
            float(rng)
        } else {
            rng.range(-1000.0..1000.0)
        }
    };
    Vec2::new(coordinate(), coordinate())
}

fn fade(rng: &mut Pcg32) -> u32 {
    match rng.range(0..5u32) {
        0 => 0,
        1 => u32::MAX,
        _ => rng.range(0..500u32),
    }
}

fn bus(rng: &mut Pcg32) -> Bus {
    Bus::ALL[rng.range(0..4u32) as usize]
}

fn generate(rng: &mut Pcg32) -> Case {
    let voices = [1, 2, 4, 48][rng.range(0..4u32) as usize];
    let commands = [2, 16, 1024][rng.range(0..3u32) as usize];
    let len = rng.range(1..80u32);
    let actions = (0..len)
        .map(|_| match rng.range(0..20u32) {
            0..=5 => Action::Play {
                sound: rng.range(0..SOUNDS as u32) as usize,
                params: PlayParams {
                    bus: bus(rng),
                    volume: float(rng),
                    pitch: float(rng),
                    pan: float(rng),
                    looped: rng.range(0..2u32) == 0,
                    priority: rng.next_u32() as u8,
                    max_instances: rng.range(0..6u32) as u8,
                },
                at: (rng.range(0..2u32) == 0).then(|| position(rng)),
            },
            6 => Action::Stop {
                voice: rng.next_u32() as usize,
                fade_ms: fade(rng),
            },
            7 | 8 => Action::Set {
                voice: rng.next_u32() as usize,
                change: match rng.range(0..4u32) {
                    0 => VoiceChange::Volume(float(rng)),
                    1 => VoiceChange::Pitch(float(rng)),
                    2 => VoiceChange::Pan(float(rng)),
                    _ => VoiceChange::Position(position(rng)),
                },
            },
            9 => Action::BusVolume {
                bus: bus(rng),
                volume: float(rng),
                fade_ms: fade(rng),
            },
            10 => Action::Listener(position(rng)),
            11 => Action::Music {
                track: rng.range(0..2u32) as usize,
                crossfade_ms: fade(rng),
            },
            12 => Action::StopMusic(fade(rng)),
            13 => Action::Update,
            _ => Action::Render {
                frames: if rng.range(0..8u32) == 0 {
                    rng.range(0..5000u32) as usize
                } else {
                    rng.range(0..700u32) as usize
                },
                channels: rng.range(0..5u32) as usize,
                rate: [
                    0,
                    1,
                    8000,
                    22_050,
                    44_100,
                    48_000,
                    96_000,
                    384_000,
                    u32::MAX,
                ][rng.range(0..9u32) as usize],
            },
        })
        .collect();
    Case {
        voices,
        commands,
        actions,
    }
}

const SOUNDS: usize = 6;

fn sounds() -> Vec<Arc<SoundData>> {
    let mut rng = Pcg32::new(7, 7);
    let mut noise = |len: usize| -> Vec<f32> { (0..len).map(|_| rng.range(-1.0..1.0)).collect() };
    [
        SoundData::new(1, 48_000, vec![1.0]),
        SoundData::new(2, 8_000, [-1.0, 1.0].repeat(7)),
        SoundData::new(1, 44_100, noise(300)),
        SoundData::new(2, 384_000, noise(5000 * 2)),
        SoundData::new(1, 1, noise(3)),
        SoundData::new(2, 48_000, [1.0, -1.0].repeat(2000)),
    ]
    .into_iter()
    .map(|sound| Arc::new(sound.expect("a valid sound")))
    .collect()
}

fn tracks() -> Vec<Arc<MusicData>> {
    let mut rng = Pcg32::new(9, 9);
    [(1u8, 8_000u32, 6000u32), (2, 384_000, 5121)]
        .into_iter()
        .map(|(channels, sample_rate, samples)| {
            let header = qoa::Header {
                channels,
                sample_rate,
                samples,
            };
            let pcm: Vec<i16> = (0..samples * u32::from(channels))
                .map(|_| rng.next_u32() as i16)
                .collect();
            let file = qoa::encode(&header, &pcm).expect("a valid track");
            Arc::new(MusicData::new(file).expect("a valid track"))
        })
        .collect()
}

fn run(case: &Case) -> Result<(), String> {
    let sounds = sounds();
    let tracks = tracks();
    let (audio, mut mixer) = nadir_audio::new(AudioConfig {
        voices: case.voices,
        commands: case.commands,
        ..AudioConfig::default()
    })
    .map_err(|error| error.to_string())?;
    let mut voices = Vec::new();
    let render = |mixer: &mut nadir_audio::Mixer, frames: usize, channels: usize, rate| {
        let mut out = vec![f32::NAN; frames * channels.max(1) + 1];
        mixer.render(&mut out, channels, rate);
        match out.iter().position(|x| !(x.is_finite() && x.abs() < 1.0)) {
            Some(at) => Err(format!(
                "sample {at} of {frames} × {channels} at {rate}: {}",
                out[at]
            )),
            None => Ok(()),
        }
    };
    for action in &case.actions {
        match *action {
            Action::Play { sound, params, at } => voices.push(match at {
                Some(at) => audio.play_at(&sounds[sound], at, params),
                None => audio.play(&sounds[sound], params),
            }),
            Action::Stop { voice, fade_ms } => {
                if let Some(&voice) = voices.get(voice % voices.len().max(1)) {
                    audio.stop(voice, fade_ms);
                }
            }
            Action::Set { voice, change } => {
                if let Some(&voice) = voices.get(voice % voices.len().max(1)) {
                    audio.set_voice(voice, change);
                }
            }
            Action::BusVolume {
                bus,
                volume,
                fade_ms,
            } => audio.set_bus_volume(bus, volume, fade_ms),
            Action::Listener(at) => audio.set_listener(at),
            Action::Music {
                track,
                crossfade_ms,
            } => audio.play_music(&tracks[track], crossfade_ms),
            Action::StopMusic(fade_ms) => audio.stop_music(fade_ms),
            Action::Update => audio.update(),
            Action::Render {
                frames,
                channels,
                rate,
            } => render(&mut mixer, frames, channels, rate)?,
        }
    }
    // Everything stopped: every sound and track comes back.
    for _ in 0..4 {
        render(&mut mixer, 512, 2, 48_000)?;
        audio.update();
    }
    for &voice in &voices {
        audio.stop(voice, 0);
        render(&mut mixer, 1, 2, 48_000)?;
    }
    audio.stop_music(0);
    for _ in 0..4 {
        render(&mut mixer, 512, 2, 48_000)?;
        audio.update();
    }
    if audio.stats().active_voices != 0 {
        return Err(format!(
            "{} voices still playing",
            audio.stats().active_voices
        ));
    }
    let held = |count: usize| count != 1;
    if sounds.iter().map(Arc::strong_count).any(held)
        || tracks.iter().map(Arc::strong_count).any(held)
    {
        return Err("a sound or a track never came back".into());
    }
    Ok(())
}

#[test]
fn whatever_the_game_asks_the_output_stays_finite_and_below_full_scale() {
    check("mixer sequences", 300, generate, run);
}
