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

//! The sound pipeline: WAVs below `audio/` cooked to `.kpcm` or `.qoa`, by
//! their sidecars or their folders, resampled to 48 kHz unless they say
//! otherwise; what it refuses, and what it warns of.

use std::fs;

use nadir_codec::wav::ErrorKind as WavError;
use nadir_codec::{CodecError, Limits, kpcm, pcm, qoa};
use nadir_cook::{CookError, Counts, SoundEncoding};

use crate::common::{
    cook, dir, files, jobs, names, round_trip, tone16, wav_bytes, wav_float, wav16, write,
};

fn kpcm_at(out: &std::path::Path, path: &str) -> kpcm::Sound {
    let bytes = fs::read(out.join(path)).unwrap_or_else(|error| panic!("{path}: {error}"));
    kpcm::decode(&bytes, Limits::default()).unwrap_or_else(|error| panic!("{path}: {error}"))
}

fn qoa_at(out: &std::path::Path, path: &str) -> qoa::Sound {
    let bytes = fs::read(out.join(path)).unwrap_or_else(|error| panic!("{path}: {error}"));
    qoa::decode(&bytes, Limits::default()).unwrap_or_else(|error| panic!("{path}: {error}"))
}

fn sound(encoding: SoundEncoding, frames: usize, sample_rate: u32) -> Option<Counts> {
    Some(Counts::Sound {
        encoding,
        frames,
        sample_rate,
    })
}

#[test]
fn sounds_below_audio_cook_to_pcm16_at_their_paths_sample_for_sample() {
    let root = dir("sounds_pcm16");
    let (source, out) = (root.join("source"), root.join("out"));
    let step = tone16(4800, 48_000, 440.0, 0.5);
    let mut stereo = Vec::new();
    for (left, right) in tone16(960, 48_000, 300.0, 0.25)
        .into_iter()
        .zip(tone16(960, 48_000, 700.0, 0.75))
    {
        stereo.extend([left, right]);
    }
    // Both end at 0, as a sound that does not loop should.
    let mut step = step;
    step.push(0);
    stereo.extend([0, 0]);
    write(&source, "audio/sfx/step.wav", wav16(1, 48_000, &step));
    write(&source, "audio/ui/Click.WAV", wav16(2, 48_000, &stereo));
    write(&source, "sfx/elsewhere.wav", wav16(1, 48_000, &step));
    write(&source, "audiobook/ch1.wav", wav16(1, 48_000, &step));
    let report = cook(&source, &out).expect("a cook");

    assert_eq!(
        jobs(&report),
        [
            ("audio/sfx/step".to_string(), true),
            ("audio/ui/click".to_string(), true)
        ]
    );
    assert_eq!(
        report.jobs[0].counts,
        sound(SoundEncoding::Pcm16, 4801, 48_000)
    );
    assert_eq!(
        report.jobs[1].counts,
        sound(SoundEncoding::Pcm16, 961, 48_000)
    );
    assert_eq!(report.warnings, Vec::<String>::new());
    let elsewhere = |path: &str| {
        (
            path.to_string(),
            "a sound is cooked only below `audio/`".to_string(),
        )
    };
    assert_eq!(
        report.skipped,
        [
            elsewhere("audiobook/ch1.wav"),
            elsewhere("sfx/elsewhere.wav")
        ]
    );
    // At the device's rate, 16-bit sounds keep every sample.
    let cooked = kpcm_at(&out, "audio/sfx/step.kpcm");
    assert_eq!(
        cooked.header,
        kpcm::Header {
            channels: 1,
            sample_rate: 48_000,
            frames: 4801
        }
    );
    assert_eq!(cooked.samples, step);
    assert_eq!(kpcm_at(&out, "audio/ui/click.kpcm").samples, stereo);
    assert_eq!(round_trip(&out), 2);
}

#[test]
fn music_cooks_to_qoa_and_loops_and_a_sidecar_says_otherwise() {
    let root = dir("sounds_music");
    let (source, out) = (root.join("source"), root.join("out"));
    // A loop of exactly 110 turns of 441 Hz at 44.1 kHz.
    let theme = tone16(11_000, 44_100, 441.0, 0.5);
    write(&source, "audio/music/theme.wav", wav16(1, 44_100, &theme));
    // Music loops whatever its format, so it may end loud.
    let mut jingle = vec![0; 479];
    jingle.push(16384);
    write(&source, "audio/music/jingle.wav", wav16(1, 48_000, &jingle));
    write(
        &source,
        "audio/music/jingle.sound.jsonc",
        r#"{ "format": "pcm16" }"#,
    );
    // Beside the music folder, not in it.
    write(&source, "audio/musicbox.wav", wav16(1, 48_000, &[0; 48]));
    write(
        &source,
        "audio/sfx/thunder.wav",
        wav16(1, 48_000, &[0; 960]),
    );
    write(
        &source,
        "audio/sfx/thunder.sound.jsonc",
        "// Long, so smaller as QOA.\n{ \"format\": \"qoa\" }",
    );
    let report = cook(&source, &out).expect("a cook");

    assert_eq!(
        jobs(&report),
        [
            ("audio/music/jingle".to_string(), true),
            ("audio/music/theme".to_string(), true),
            ("audio/musicbox".to_string(), true),
            ("audio/sfx/thunder".to_string(), true)
        ]
    );
    // 11,000 frames at 44.1 kHz are 11,973 at 48.
    assert_eq!(
        report.jobs[1].counts,
        sound(SoundEncoding::Qoa, 11_973, 48_000)
    );
    assert_eq!(report.warnings, Vec::<String>::new());
    assert_eq!(
        names(&out),
        [
            ".cache.kbj",
            "audio/music/jingle.kpcm",
            "audio/music/theme.qoa",
            "audio/musicbox.kpcm",
            "audio/sfx/thunder.qoa",
            "game.kpak"
        ]
    );
    assert_eq!(
        qoa_at(&out, "audio/music/theme.qoa").header,
        qoa::Header {
            channels: 1,
            sample_rate: 48_000,
            samples: 11_973
        }
    );
    // Resampled as a loop, which a sound that does not loop is not: the
    // codec's resampler, its rounding and QOA's encoder, byte for byte.
    let as_f32: Vec<f32> = theme.iter().map(|&s| f32::from(s) / 32768.0).collect();
    let encoded = |looped| {
        let samples: Vec<i16> =
            pcm::resample(&as_f32, 1, 44_100, 48_000, looped, Limits::default())
                .expect("a resample")
                .into_iter()
                .map(pcm::to_i16)
                .collect();
        let header = qoa::Header {
            channels: 1,
            sample_rate: 48_000,
            samples: 11_973,
        };
        qoa::encode(&header, &samples).expect("an encoding")
    };
    let cooked = fs::read(out.join("audio/music/theme.qoa")).expect("the theme");
    assert_eq!(cooked, encoded(true));
    assert_ne!(cooked, encoded(false));
}

#[test]
fn a_sound_is_resampled_to_48_khz_unless_its_sidecar_gives_a_rate() {
    let root = dir("sounds_rate");
    let (source, out) = (root.join("source"), root.join("out"));
    let mut wind = tone16(4410, 44_100, 500.0, 0.3);
    wind.push(0);
    write(&source, "audio/ambience/wind.wav", wav16(1, 44_100, &wind));
    write(&source, "audio/ambience/rain.wav", wav16(1, 44_100, &wind));
    write(
        &source,
        "audio/ambience/rain.sound.jsonc",
        r#"{ "rate": 22050, "loop": true }"#,
    );
    let report = cook(&source, &out).expect("a cook");

    let frames = |to| pcm::resampled_frames(4411, 44_100, to).expect("rates") as usize;
    assert_eq!(
        report.jobs[0].counts,
        sound(SoundEncoding::Pcm16, frames(22_050), 22_050)
    );
    assert_eq!(
        report.jobs[1].counts,
        sound(SoundEncoding::Pcm16, frames(48_000), 48_000)
    );
    // The codec's resampler, then its rounding, as the game's would be.
    let as_f32: Vec<f32> = wind.iter().map(|&s| f32::from(s) / 32768.0).collect();
    for (path, rate, looped) in [
        ("audio/ambience/wind.kpcm", 48_000, false),
        ("audio/ambience/rain.kpcm", 22_050, true),
    ] {
        let expected: Vec<i16> = pcm::resample(&as_f32, 1, 44_100, rate, looped, Limits::default())
            .expect("a resample")
            .into_iter()
            .map(pcm::to_i16)
            .collect();
        let cooked = kpcm_at(&out, path);
        assert_eq!(cooked.header.sample_rate, rate, "{path}");
        assert_eq!(cooked.samples, expected, "{path}");
    }
}

#[test]
fn deep_and_float_samples_round_to_16_bits_and_overs_are_clipped_and_told() {
    let root = dir("sounds_float");
    let (source, out) = (root.join("source"), root.join("out"));
    // Full scale, ±1, is not past it.
    let samples = [0.5f32, -0.25, 1.5, -2.0, 1.0, -1.0, 0.0];
    write(&source, "audio/hit.wav", wav_float(1, 48_000, &samples));
    // 24 bits: 0x123456 and −0x400000, then 0.
    let deep = [0x56, 0x34, 0x12, 0x00, 0x00, 0xc0, 0, 0, 0];
    write(
        &source,
        "audio/deep.wav",
        wav_bytes(1, 1, 48_000, 24, &deep),
    );
    let report = cook(&source, &out).expect("a cook");

    assert_eq!(
        kpcm_at(&out, "audio/hit.kpcm").samples,
        [16384, -8192, 32767, -32768, 32767, -32768, 0]
    );
    // 0x123456 / 2²³ × 2¹⁵ = 0x1234.56: rounded to 0x1234.
    assert_eq!(
        kpcm_at(&out, "audio/deep.kpcm").samples,
        [0x1234, -0x4000, 0]
    );
    assert_eq!(
        report.warnings,
        ["`audio/hit.wav` has 2 samples past full scale, which are clipped: lower its level"]
    );
}

#[test]
fn a_sound_that_ends_loud_or_lasts_long_as_pcm16_is_told() {
    let root = dir("sounds_warnings");
    let (source, out) = (root.join("source"), root.join("out"));
    // Ends at −6 dBFS.
    write(&source, "audio/a/bang.wav", wav16(1, 48_000, &[0, 16384]));
    // Ends at 0, though resampled it would not: what the cook says is of
    // the WAV's own end.
    let mut chirp = tone16(4410, 44_100, 3000.0, 0.9);
    chirp.push(0);
    write(&source, "audio/a/chirp.wav", wav16(1, 44_100, &chirp));
    // Ends at full scale, which is 0 dBFS, not −0.
    write(&source, "audio/a/full.wav", wav16(1, 48_000, &[0, 32767]));
    // Ends at −12 dBFS in one channel, then the other.
    write(
        &source,
        "audio/a/left.wav",
        wav16(2, 48_000, &[0, 0, -8192, 0]),
    );
    write(
        &source,
        "audio/a/right.wav",
        wav16(2, 48_000, &[0, 0, 0, -8192]),
    );
    // The same, but it loops, so its end leads into its start.
    write(&source, "audio/a/hum.wav", wav16(1, 48_000, &[0, 16384]));
    write(&source, "audio/a/hum.sound.jsonc", r#"{ "loop": true }"#);
    // Ten seconds of silence: as PCM16 by default, but QOA would do.
    write(
        &source,
        "audio/a/rain.wav",
        wav16(1, 48_000, &vec![0; 480_000]),
    );
    // The same, kept as PCM16 by its sidecar's choice.
    write(
        &source,
        "audio/a/drone.wav",
        wav16(1, 48_000, &vec![0; 480_000]),
    );
    write(
        &source,
        "audio/a/drone.sound.jsonc",
        r#"{ "format": "pcm16" }"#,
    );
    let report = cook(&source, &out).expect("a cook");

    let ends = |name: &str, level: &str| {
        format!(
            "`audio/a/{name}.wav` ends at {level} dBFS, so it clicks when it stops: end it at 0, \
             or give its sidecar `\"loop\": true` if it loops"
        )
    };
    assert_eq!(
        report.warnings,
        [
            ends("bang", "-6"),
            ends("full", "0"),
            ends("left", "-12"),
            "`audio/a/rain.wav` lasts 10.0 s: give its sidecar `\"format\": \"qoa\"` for a file \
             about a fifth the size"
                .to_string(),
            ends("right", "-12"),
        ]
    );
}

#[test]
fn what_cannot_be_cooked_is_named_and_nothing_is_written_for_a_sidecar() {
    // Each test's name, its source files, and the file its error names.
    type Case<'a> = (&'a str, &'a [(&'a str, Vec<u8>)], &'a str);
    let cases: [Case<'_>; 3] = [
        (
            "bad_sidecar",
            &[
                ("audio/a.wav", wav16(1, 48_000, &[0])),
                ("audio/a.sound.jsonc", br#"{ "speed": 2 }"#.to_vec()),
            ],
            "audio/a.sound.jsonc",
        ),
        (
            "orphan",
            &[("audio/sfx/Gone.sound.jsonc", b"{}".to_vec())],
            "audio/sfx/Gone.sound.jsonc",
        ),
        (
            "rate_word",
            &[
                ("audio/a.wav", wav16(1, 48_000, &[0])),
                ("audio/a.sound.jsonc", br#"{ "rate": "fast" }"#.to_vec()),
            ],
            "audio/a.sound.jsonc",
        ),
    ];
    for (test, sources, named) in cases {
        let root = dir(&format!("sounds_{test}"));
        let (source, out) = (root.join("source"), root.join("out"));
        for (path, bytes) in sources {
            write(&source, path, bytes);
        }
        let result = cook(&source, &out);
        assert!(
            matches!(&result, Err(CookError::Config { path, .. }) if path == named),
            "{test}: {result:?}"
        );
        assert!(!out.exists(), "{test}: nothing written");
    }
    let root = dir("sounds_orphan_message");
    let (source, out) = (root.join("source"), root.join("out"));
    write(&source, "audio/sfx/Gone.sound.jsonc", "{}");
    assert_eq!(
        cook(&source, &out).map(drop),
        Err(CookError::Config {
            path: "audio/sfx/Gone.sound.jsonc".to_string(),
            reason: "there is no `gone.wav` beside it".to_string()
        })
    );
}

#[test]
fn a_wav_the_cook_cannot_take_is_named() {
    let one_hertz: Vec<i16> = vec![0; 1400];
    let cases: [(&str, Vec<u8>, Option<&str>, &str); 5] = [
        (
            "eight_bit",
            wav_bytes(1, 1, 8000, 8, &[128, 129]),
            None,
            "decode",
        ),
        ("empty", wav16(2, 48_000, &[]), None, "it has no samples"),
        (
            "too_fast",
            wav16(1, 384_001, &[0, 0]),
            None,
            "its rate of 384001 Hz is above the 384000 Hz the game plays",
        ),
        (
            "too_long",
            wav16(1, 1, &one_hertz),
            Some(r#"{ "rate": 384000 }"#),
            "resampled to 384000 Hz, it would take more than 512 MiB",
        ),
        // 11,185 frames at 1 Hz are 4,295,040,000 at 384 kHz.
        (
            "too_many_frames",
            wav16(1, 1, &[0; 11_185]),
            Some(r#"{ "rate": 384000 }"#),
            "resampled to 384000 Hz, it would have 4295040000 frames, more than a sound holds",
        ),
    ];
    for (test, bytes, sidecar, reason) in cases {
        let root = dir(&format!("sounds_{test}"));
        let (source, out) = (root.join("source"), root.join("out"));
        write(&source, "audio/x.wav", bytes);
        if let Some(sidecar) = sidecar {
            write(&source, "audio/x.sound.jsonc", sidecar);
        }
        let result = cook(&source, &out).map(drop);
        if reason == "decode" {
            // jsfxr writes 8-bit WAVs unless asked otherwise.
            assert!(
                matches!(
                    &result,
                    Err(CookError::Decode {
                        path,
                        error: CodecError::Wav {
                            kind: WavError::UnsupportedFormat { tag: 1, bits: 8 },
                            ..
                        }
                    }) if path == "audio/x.wav"
                ),
                "{test}: {result:?}"
            );
        } else {
            assert_eq!(
                result,
                Err(CookError::Config {
                    path: "audio/x.wav".to_string(),
                    reason: reason.to_string()
                }),
                "{test}"
            );
        }
    }
}

#[test]
fn outputs_and_names_that_cannot_be_are_refused_before_anything_is_written() {
    let root = dir("sounds_collision");
    let (source, out) = (root.join("source"), root.join("out"));
    write(&source, "audio/a.wav", wav16(1, 48_000, &[0]));
    write(&source, "audio/a.kpcm/b.wav", wav16(1, 48_000, &[0]));
    assert!(matches!(
        cook(&source, &out),
        Err(CookError::OutputCollision { .. })
    ));
    assert!(!out.exists());

    let root = dir("sounds_dot");
    let (source, out) = (root.join("source"), root.join("out"));
    write(&source, "audio/a..wav", wav16(1, 48_000, &[0]));
    assert!(matches!(
        cook(&source, &out),
        Err(CookError::BadPath { path, .. }) if path == "audio/a..wav"
    ));
    assert!(!out.exists());
}

#[test]
fn each_sound_is_cooked_again_alone_and_the_cook_owns_the_audio_folder() {
    let root = dir("sounds_changed");
    let (source, out) = (root.join("source"), root.join("out"));
    write(&source, "audio/a.wav", wav16(1, 48_000, &[1, 2, 0]));
    write(&source, "audio/b.wav", wav16(1, 48_000, &[3, 4, 0]));
    cook(&source, &out).expect("a cook");
    let first = files(&out);
    assert_eq!(
        jobs(&cook(&source, &out).expect("a cook")),
        [
            ("audio/a".to_string(), false),
            ("audio/b".to_string(), false)
        ]
    );
    assert_eq!(files(&out), first);
    // A sidecar is an input of its sound's job.
    write(&source, "audio/b.sound.jsonc", r#"{ "format": "qoa" }"#);
    assert_eq!(
        jobs(&cook(&source, &out).expect("a cook")),
        [
            ("audio/a".to_string(), false),
            ("audio/b".to_string(), true)
        ]
    );
    // And so is what it says.
    write(
        &source,
        "audio/b.sound.jsonc",
        r#"{ "format": "qoa", "rate": 24000 }"#,
    );
    assert_eq!(
        jobs(&cook(&source, &out).expect("a cook")),
        [
            ("audio/a".to_string(), false),
            ("audio/b".to_string(), true)
        ]
    );
    assert_eq!(qoa_at(&out, "audio/b.qoa").header.sample_rate, 24_000);
    // A file no job wrote, the old output of a sound whose format changed,
    // and a removed source's output, go.
    write(&out, "audio/stray.kpcm", "stray");
    write(&out, "elsewhere/kept.txt", "not the cook's");
    fs::remove_file(source.join("audio/a.wav")).expect("remove a source");
    cook(&source, &out).expect("a cook");
    assert_eq!(
        names(&out),
        [
            ".cache.kbj",
            "audio/b.qoa",
            "elsewhere/kept.txt",
            "game.kpak"
        ]
    );
    let clean = root.join("clean");
    cook(&source, &clean).expect("a cook");
    assert_eq!(
        files(&out)
            .into_iter()
            .filter(|(name, _)| name != "elsewhere/kept.txt")
            .collect::<Vec<_>>(),
        files(&clean)
    );
    assert_eq!(round_trip(&clean), 1);
}
