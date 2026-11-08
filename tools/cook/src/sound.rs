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
/// one the engine's audio output opens at.
pub(crate) const RATE: u32 = 48_000;

/// How long a sound cooked as PCM16 may last, by default, before the cook
/// suggests QOA.
const LONG_SECONDS: u64 = 10;

/// The loudest a sound that does not loop may end, as a 16-bit sample: about
/// −48 dBFS. Louder, it clicks when it stops.
const QUIET_END: i32 = 128;

/// The limits WAVs are read within: the default, but for the samples,
/// which may take 512 MiB as `f32`, 23 minutes of 48 kHz stereo. The
/// resampled samples are held to the same.
pub(crate) fn source_limits() -> Limits {
    Limits {
        max_decoded_bytes: 512 << 20,
        ..Limits::default()
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
///   384,000 Hz, or of more frames than a sound can hold, resampled, or that
///   as PCM16 would take more than the game reads;
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
    if settings.encoding == SoundEncoding::Pcm16 {
        let file_size = kpcm::HEADER_SIZE as u64 + out_frames * channels as u64 * 2;
        let game = Limits::default().max_decoded_bytes as u64;
        if file_size > game {
            return Err(config(format!(
                "as PCM16 it would take more than the {} MiB the game reads: give its sidecar \
                 `\"format\": \"qoa\"`",
                game >> 20
            )));
        }
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
            read_back_qoa(&file, &header).map_err(|error| CookError::Encode {
                path: output.to_string(),
                error,
            })?;
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
            // The game's reader, with the game's limits, as levels are read
            // back: the cook never writes a file the game refuses.
            let read =
                kpcm::decode(&file, Limits::default()).map_err(|error| CookError::Encode {
                    path: output.to_string(),
                    error,
                })?;
            debug_assert_eq!(read.samples, samples, "PCM16 is lossless");
            file
        }
    };
    Ok(Cooked {
        file,
        frames: out_frames as usize,
        warnings,
    })
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
