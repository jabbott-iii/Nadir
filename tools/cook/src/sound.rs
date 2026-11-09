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

//! The sound pipeline: a WAV below `audio/` to a sound file, `.kpcm`, or
//! QOA, `.qoa`, at its path, as docs/subsystems/assets-and-codecs.md's
//! "Sounds" says.

use nadir_codec::wav::{self, Wave};
use nadir_codec::{Limits, kpcm, pcm, qoa};

use crate::config;
use crate::{CookError, SoundEncoding};

/// The folder whose sounds are music: QOA and looped, unless their sidecars
/// say otherwise.
pub(crate) const MUSIC_FOLDER: &str = "audio/music/";

/// The rate sounds are cooked to, unless their sidecars say otherwise: the
/// one the engine's audio output opens at, and the one the game's `Sound`
/// converts a sound at another rate to as it loads, if it ends in quiet.
pub(crate) const RATE: u32 = 48_000;

/// How long a sound cooked as PCM16 may last, by default, before the cook
/// suggests QOA.
const LONG_SECONDS: u64 = 10;

/// The loudest a sound that does not loop may end, as a 16-bit sample: about
/// −48 dBFS. Louder, it clicks when it stops.
const QUIET_END: i32 = 128;

/// The frames at a sound's end that the game's `Sound` finds within
/// [`QUIET_END`] before it converts it as it loads: those of its last 2 ms,
/// and at least this many.
const QUIET_FRAMES: usize = 36;

/// What to do with a sound too long for the game to hold.
const TOO_LONG: &str = "shorten it, or make it music, QOA below `audio/music/`, which the game \
                        decodes as it plays";

/// The limits WAVs are read within: the default, but for the samples,
/// which may take 512 MiB as `f32`, 23 minutes of 48 kHz stereo. The
/// resampled samples are held to the same.
pub(crate) fn source_limits() -> Limits {
    Limits {
        max_decoded_bytes: 512 << 20,
        ..Limits::default()
    }
}

/// The limits the game's `Sound` decodes a sound's 16-bit samples within:
/// half the default's bytes, so that their `f32` copy, 4 bytes a sample,
/// is held to the default's whole.
fn sound_limits() -> Limits {
    let limits = Limits::default();
    Limits {
        max_decoded_bytes: limits.max_decoded_bytes / 2,
        ..limits
    }
}

/// How a sound is cooked: its sidecar's keys, or the defaults of where it
/// is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Settings {
    pub(crate) encoding: SoundEncoding,
    /// Whether the sidecar gave the encoding.
    pub(crate) chosen: bool,
    pub(crate) rate: u32,
    pub(crate) looped: bool,
}

impl Settings {
    /// The settings of the sound at `path`, lowercased, from its sidecar's.
    pub(crate) fn of(path: &str, sidecar: &config::Sound) -> Self {
        let music = path.starts_with(MUSIC_FOLDER);
        let default = if music {
            SoundEncoding::Qoa
        } else {
            SoundEncoding::Pcm16
        };
        Self {
            encoding: sidecar.format.unwrap_or(default),
            chosen: sidecar.format.is_some(),
            rate: sidecar.rate.unwrap_or(RATE),
            looped: sidecar.looped.unwrap_or(music),
        }
    }
}

/// The output of the sound whose job is `job`, `audio/<path>`, in
/// `encoding`.
pub(crate) fn output(job: &str, encoding: SoundEncoding) -> String {
    match encoding {
        SoundEncoding::Qoa => format!("{job}.qoa"),
        _ => format!("{job}.kpcm"),
    }
}

/// What a sound cooks to.
pub(crate) struct Cooked {
    /// The file's bytes.
    pub(crate) file: Vec<u8>,
    /// Its frames, at the rate cooked to.
    pub(crate) frames: usize,
    /// What the user should know, naming the WAV.
    pub(crate) warnings: Vec<String>,
}

/// Cooks the WAV `bytes`, of the sound `found` (as found), into `output`.
///
/// # Errors
///
/// - [`CookError::Decode`] for a WAV the reader refuses, as an 8-bit one;
/// - [`CookError::Config`] for one of no samples, of a rate above
///   384,000 Hz, or of more frames than a sound can hold, resampled, or a
///   sound, not music, that would take the game more than its limits to
///   hold, as it reads it or as it converts it;
/// - [`CookError::Encode`] for a file the encoder, or its reader, refuses.
pub(crate) fn cook(
    path: &str,
    found: &str,
    bytes: &[u8],
    settings: &Settings,
    output: &str,
) -> Result<Cooked, CookError> {
    let config = |reason: String| CookError::Config {
        path: found.to_string(),
        reason,
    };
    let Wave {
        sample_rate,
        channels,
        samples,
        ..
    } = wav::decode(bytes, source_limits()).map_err(|error| CookError::Decode {
        path: path.to_string(),
        error,
    })?;
    let channels = usize::from(channels);
    let frames = samples.len() / channels;
    if frames == 0 {
        return Err(config("it has no samples".to_string()));
    }
    if sample_rate > kpcm::MAX_SAMPLE_RATE {
        return Err(config(format!(
            "its rate of {sample_rate} Hz is above the {} Hz the game plays",
            kpcm::MAX_SAMPLE_RATE
        )));
    }
    // The WAV's own last frame, before the resample, which rings past it.
    let loud_end = if settings.looped {
        None
    } else {
        samples[samples.len() - channels..]
            .iter()
            .map(|&x| i32::from(pcm::to_i16(x)).abs())
            .max()
            .filter(|&loudest| loudest > QUIET_END)
    };
    let rate = settings.rate;
    let out_frames = pcm::resampled_frames(frames as u64, sample_rate, rate).unwrap_or(0);
    let Ok(out_frames_u32) = u32::try_from(out_frames) else {
        return Err(config(format!(
            "resampled to {rate} Hz, it would have {out_frames} frames, more than a sound holds"
        )));
    };
    let limit = source_limits().max_decoded_bytes as u64;
    if out_frames * channels as u64 * 4 > limit {
        return Err(config(format!(
            "resampled to {rate} Hz, it would take more than {} MiB",
            limit >> 20
        )));
    }
    // Music, QOA below `audio/music/`, is held compressed: the file of
    // 512 MiB of `f32` samples is about 52 MiB, far below the 256 MiB the
    // game's `Music` reads. The game's `Sound` holds every sample as an
    // `f32`.
    let music = settings.encoding == SoundEncoding::Qoa && path.starts_with(MUSIC_FOLDER);
    if !music && !fits_the_game(out_frames * channels as u64) {
        return Err(config(format!(
            "as a sound, it would take the game more than {} MiB, 4 bytes a sample: {TOO_LONG}",
            Limits::default().max_decoded_bytes >> 20
        )));
    }
    let samples = if sample_rate == rate {
        samples
    } else {
        let resampled = pcm::resample(
            &samples,
            channels,
            sample_rate,
            rate,
            settings.looped,
            source_limits(),
        )
        .map_err(|error| CookError::Encode {
            path: output.to_string(),
            error,
        })?;
        // The WAV's samples, no longer needed: at most half the memory
        // the job holds at once.
        drop(samples);
        resampled
    };

    let mut warnings = Vec::new();
    let clipped = samples.iter().filter(|x| x.abs() > 1.0).count();
    if clipped > 0 {
        let after = if sample_rate == rate {
            String::new()
        } else {
            format!(" after resampling to {rate} Hz")
        };
        warnings.push(format!(
            "`{found}` has {clipped} samples past full scale{after}, which are clipped: lower \
             its level"
        ));
    }
    let samples: Vec<i16> = samples.into_iter().map(pcm::to_i16).collect();
    if let Some(loudest) = loud_end {
        // Rounded, and +0 where that is −0.
        let level = (20.0 * (f64::from(loudest) / 32768.0).log10()).round() + 0.0;
        warnings.push(format!(
            "`{found}` ends at {level:.0} dBFS, so it clicks when it stops: end it at 0, or \
             give its sidecar `\"loop\": true` if it loops"
        ));
    }
    let channels_u8 = channels as u8;
    let file = match settings.encoding {
        SoundEncoding::Qoa => {
            let header = qoa::Header {
                channels: channels_u8,
                sample_rate: rate,
                samples: out_frames_u32,
            };
            let file = qoa::encode(&header, &samples).map_err(|error| CookError::Encode {
                path: output.to_string(),
                error,
            })?;
            if music {
                read_back_qoa(&file, &header).map_err(|error| CookError::Encode {
                    path: output.to_string(),
                    error,
                })?;
            } else {
                // As the game's `Sound` reads it: whole, within its limits.
                let read =
                    qoa::decode(&file, sound_limits()).map_err(|error| CookError::Encode {
                        path: output.to_string(),
                        error,
                    })?;
                debug_assert_eq!(read.header, header, "the encoder wrote the header given");
                check_conversion(
                    &read.samples,
                    usize::from(read.header.channels),
                    read.header.sample_rate,
                )
                .map_err(config)?;
            }
            file
        }
        _ => {
            let long = out_frames >= LONG_SECONDS * u64::from(rate);
            if long && !settings.chosen {
                warnings.push(format!(
                    "`{found}` lasts {:.1} s: give its sidecar `\"format\": \"qoa\"` for a \
                     file about a fifth the size",
                    out_frames as f64 / f64::from(rate)
                ));
            }
            let header = kpcm::Header {
                channels: channels_u8,
                sample_rate: rate,
                frames: out_frames_u32,
            };
            let file = kpcm::encode(&header, &samples).map_err(|error| CookError::Encode {
                path: output.to_string(),
                error,
            })?;
            // The game's reader, with the limits the game's `Sound` reads it
            // within, as levels are read back: the cook never writes a file
            // the game refuses.
            let read = kpcm::decode(&file, sound_limits()).map_err(|error| CookError::Encode {
                path: output.to_string(),
                error,
            })?;
            debug_assert_eq!(read.samples, samples, "PCM16 is lossless");
            check_conversion(
                &read.samples,
                usize::from(read.header.channels),
                read.header.sample_rate,
            )
            .map_err(config)?;
            file
        }
    };
    Ok(Cooked {
        file,
        frames: out_frames as usize,
        warnings,
    })
}

/// Whether the game's `Sound` converts `samples`, 16-bit and interleaved, of
/// `channels` at `rate`, to [`RATE`] as it loads them: at another rate, as
/// every rate a sidecar gives is one it converts from, if their last 2 ms,
/// and [`QUIET_FRAMES`] at least, are within [`QUIET_END`], as a sound that
/// does not loop ends.
///
/// nadir-audio's `Sound` (ADR-0050), restated, as the cook may not depend
/// on nadir-audio: a change there is a change here.
fn converts_as_it_loads(samples: &[i16], channels: usize, rate: u32) -> bool {
    if rate == RATE {
        return false;
    }
    let frames = (rate as usize / 500).max(QUIET_FRAMES);
    samples
        .iter()
        .rev()
        .take(frames.saturating_mul(channels))
        .all(|&sample| i32::from(sample).abs() <= QUIET_END)
}

/// Checks that the game's `Sound` can hold `decoded`, the 16-bit samples of
/// `channels` at `rate` it reads from a sound's file, once it has converted
/// them as it loads, if it does: their frames at [`RATE`], 4 bytes a sample,
/// within the default limits, as `pcm::resample` holds them. The reason, if
/// it cannot.
fn check_conversion(decoded: &[i16], channels: usize, rate: u32) -> Result<(), String> {
    if !converts_as_it_loads(decoded, channels, rate) {
        return Ok(());
    }
    let frames = (decoded.len() / channels) as u64;
    let converted = pcm::resampled_frames(frames, rate, RATE).unwrap_or(u64::MAX);
    if !fits_the_game(converted.saturating_mul(channels as u64)) {
        return Err(format!(
            "it ends in quiet, so the game converts it from {rate} Hz to {RATE} Hz as it loads, \
             which would take more than {} MiB, 4 bytes a sample: {TOO_LONG}",
            Limits::default().max_decoded_bytes >> 20
        ));
    }
    Ok(())
}

/// Whether the game's `Sound` can hold `samples`, as `f32`, within the
/// default limits: as it decodes their 16 bits within [`sound_limits`].
fn fits_the_game(samples: u64) -> bool {
    samples.saturating_mul(4) <= Limits::default().max_decoded_bytes as u64
}

/// Reads a QOA file back as the game's music does, a frame at a time, with
/// nothing more allocated than a frame: every frame decodes, and the file
/// holds `header`'s samples.
fn read_back_qoa(file: &[u8], header: &qoa::Header) -> Result<(), nadir_codec::CodecError> {
    let read = qoa::header(file)?;
    debug_assert_eq!(read, *header, "the encoder wrote the header given");
    let mut frame = vec![0; qoa::FRAME_LEN * usize::from(read.channels)];
    // The file's header, `qoaf` and the samples, is 8 bytes.
    let mut at = 8;
    let mut total = 0;
    while at < file.len() {
        let decoded = qoa::decode_frame(file, &read, at, &mut frame)?;
        total += decoded.samples;
        at = decoded.next;
    }
    debug_assert_eq!(total, read.samples as usize, "every frame was read");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_game_holds_a_sound_of_64_mi_samples_and_no_more() {
        // 256 MiB of `f32`.
        assert!(fits_the_game(1 << 26));
        assert!(!fits_the_game((1 << 26) + 1));
        assert!(!fits_the_game(u64::MAX));
        assert_eq!(sound_limits().max_decoded_bytes, 128 << 20);
    }

    #[test]
    fn a_sound_converts_as_it_loads_if_its_last_2_ms_and_36_frames_are_quiet() {
        // Silence but for one sample of `level`, `back` samples from the end.
        let with = |len: usize, back: usize, level: i16| {
            let mut samples = vec![0i16; len];
            samples[len - back] = level;
            samples
        };
        // At 8 kHz, 2 ms are 16 frames: the 36 hold.
        assert!(converts_as_it_loads(&with(100, 37, 30_000), 1, 8_000));
        assert!(!converts_as_it_loads(&with(100, 36, 30_000), 1, 8_000));
        // −48 dBFS is quiet, one step more is not, of either sign.
        assert!(converts_as_it_loads(&with(100, 1, 128), 1, 8_000));
        assert!(converts_as_it_loads(&with(100, 1, -128), 1, 8_000));
        assert!(!converts_as_it_loads(&with(100, 1, 129), 1, 8_000));
        assert!(!converts_as_it_loads(&with(100, 1, -129), 1, 8_000));
        // At 384 kHz, 2 ms are 768 frames.
        assert!(converts_as_it_loads(&with(1000, 769, 30_000), 1, 384_000));
        assert!(!converts_as_it_loads(&with(1000, 768, 30_000), 1, 384_000));
        // In stereo, the frames are counted, not the samples: the left
        // channel of the 36th frame from the end.
        assert!(converts_as_it_loads(&with(200, 74, 30_000), 2, 8_000));
        assert!(!converts_as_it_loads(&with(200, 72, 30_000), 2, 8_000));
        // A sound shorter than the end it needs quiet, if it is quiet.
        assert!(converts_as_it_loads(&[0; 3], 1, 8_000));
        // At the rate it loads at, nothing converts.
        assert!(!converts_as_it_loads(&[0; 100], 1, RATE));
    }

    #[test]
    fn a_sound_converted_as_it_loads_is_held_to_the_limits() {
        // 11,184,810 frames at 8 kHz are 67,108,860 at 48 kHz, 2²⁶ less 4.
        let fits = vec![0i16; 11_184_810];
        assert_eq!(check_conversion(&fits, 1, 8_000), Ok(()));
        let over = vec![0i16; 11_184_811];
        assert_eq!(
            check_conversion(&over, 1, 8_000),
            Err(format!(
                "it ends in quiet, so the game converts it from 8000 Hz to 48000 Hz as it \
                 loads, which would take more than 256 MiB, 4 bytes a sample: {TOO_LONG}"
            ))
        );
        // The same, but loud at its end, keeps its rate.
        let mut loud = over;
        *loud.last_mut().expect("samples") = 129;
        assert_eq!(check_conversion(&loud, 1, 8_000), Ok(()));
        // In stereo, 5,592,406 frames are 33,554,436 at 48 kHz, 2²⁶ and 8
        // samples.
        assert!(check_conversion(&vec![0i16; 5_592_405 * 2], 2, 8_000).is_ok());
        assert!(check_conversion(&vec![0i16; 5_592_406 * 2], 2, 8_000).is_err());
    }
}
