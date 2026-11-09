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

//! The sound and music assets, [`Sound`] and [`Music`], which the asset
//! server loads from the cook's sound files and QOA files.

use std::sync::Arc;

use nadir_assets::{Asset, AssetError, DecodeCtx, FinalizeCtx};
use nadir_codec::{Limits, kpcm, pcm, qoa};
use nadir_core::log::warn;

use crate::data::check_rate;
use crate::{AudioError, MAX_SAMPLE_RATE, MusicData, SoundData};

/// The rate sounds load at, in frames a second: the output's, as
/// `nadir_platform::AudioOutputConfig` opens it by default and every
/// backend opens at or converts to, and the cook's. A sound that loads at
/// another rate, from 8,000 to 384,000 Hz, and ends in quiet, as a sound
/// that does not loop should, is converted to it as it decodes, by
/// `nadir_codec::pcm`'s windowed sinc, so that it plays without the
/// mixer's resampling; any other keeps its own rate. A game that opens its
/// output at another rate hears the mixer resample its sounds, as it
/// resamples music.
pub const LOAD_RATE: u32 = 48_000;

/// The lowest rate a loading sound is converted from: the cook's lowest. A
/// sound below it keeps its rate, so that a file of a few kilobytes cannot
/// keep a worker converting for a minute.
const MIN_CONVERTED: u32 = 8_000;

/// The loudest a sample of a sound's quiet end may be, as 16 bits: about
/// −48 dBFS, the cook's bound for the end of a sound that does not loop.
const QUIET: u16 = 128;

/// The frames of a sound's end that must be quiet for it to be converted:
/// those of its last 2 ms, and at least this many, the resampler's reach
/// as it converts up.
const QUIET_FRAMES: usize = 36;

/// A sound, in memory, which [`Audio::play`](crate::Audio::play) plays
/// from its handle, and [`Audio::play_sound`](crate::Audio::play_sound) as
/// it is.
///
/// As an asset, it loads from a sound file, `.kpcm`, or a QOA file,
/// `.qoa`, as the cook writes them, of 1 or 2 channels at 1 to 384,000 Hz,
/// decoded on a worker, or on the caller's thread through `load_now`.
///
/// - **Converted** to [`LOAD_RATE`] from 8,000 to 384,000 Hz, if its last
///   2 ms (and 36 frames at least) are within −48 dBFS, as the cook asks of
///   a sound that does not loop: as such a sound, since the files do not
///   say. A sound that ends louder may be a loop, which that would break at
///   its seam (by −31 dBFS for a 441 Hz tone at −6 dBFS from 22,050 Hz, cut
///   at a zero), so it keeps its rate, and the mixer, which wraps a loop as
///   it plays, resamples it. Converted samples past full scale are clipped,
///   with a warning that names the file. Converting takes the worker about
///   0.4 s for 10 s of 44.1 kHz mono in an optimized build, 8 to 14 times
///   that unoptimized, and up to about a minute for a file at the limits.
/// - **Bounded:** a sound takes 4 bytes a sample, and its file 2: the
///   decoded samples are held to half the server's limits, so that its
///   `f32` samples are held to them whole, and so are the converted ones.
/// - **A failed sound's placeholder** is a frame of silence: with
///   placeholders on, it plays as silence.
#[derive(Clone, Debug)]
pub struct Sound {
    data: Arc<SoundData>,
}

impl Sound {
    /// A sound of `data`, as it is: at its own rate, not converted. The
    /// mixer resamples it, if it must, as it plays.
    pub fn new(data: SoundData) -> Self {
        Self {
            data: Arc::new(data),
        }
    }

    /// Its samples. A voice that plays it holds this `Arc` until the mixer
    /// is done with it, and [`Audio::update`](crate::Audio::update) drops
    /// it.
    pub fn data(&self) -> &Arc<SoundData> {
        &self.data
    }
}

impl Asset for Sound {
    type Decoded = SoundData;
    const EXTENSIONS: &'static [&'static str] = &["kpcm", "qoa"];

    fn decode(bytes: &[u8], ctx: &DecodeCtx<'_>) -> Result<SoundData, AssetError> {
        let limits = ctx.limits();
        // Its `f32` samples take twice the bytes of the file's 16 bits.
        let half = Limits {
            max_decoded_bytes: limits.max_decoded_bytes / 2,
            ..limits
        };
        let (channels, rate, samples) = if ctx.path().ends_with(".kpcm") {
            let sound = kpcm::decode(bytes, half).map_err(AssetError::Decode)?;
            (
                sound.header.channels,
                sound.header.sample_rate,
                sound.samples,
            )
        } else {
            // QOA takes 8 channels and rates past 384,000 Hz: refused from
            // the header, before anything is decoded.
            let header = qoa::header(bytes).map_err(AssetError::Decode)?;
            if header.channels > 2 {
                return Err(asset_error(AudioError::Channels {
                    found: header.channels.into(),
                }));
            }
            check_rate(header.sample_rate).map_err(asset_error)?;
            let sound = qoa::decode(bytes, half).map_err(AssetError::Decode)?;
            (header.channels, header.sample_rate, sound.samples)
        };
        let quiet = ends_quietly(&samples, channels, rate);
        let samples: Vec<f32> = samples
            .into_iter()
            .map(|sample| f32::from(sample) / 32768.0)
            .collect();
        let (rate, samples) = if quiet {
            convert(ctx.path(), channels, rate, samples, limits)?
        } else {
            (rate, samples)
        };
        SoundData::new(channels, rate, samples).map_err(asset_error)
    }

    fn finalize(decoded: SoundData, _ctx: &mut FinalizeCtx<'_>) -> Result<Self, AssetError> {
        Ok(Self::new(decoded))
    }

    fn placeholder(_ctx: &mut FinalizeCtx<'_>) -> Result<Self, AssetError> {
        let silence = SoundData::new(1, LOAD_RATE, vec![0.0]).map_err(asset_error)?;
        Ok(Self::new(silence))
    }
}

/// Whether `samples`, of `channels` at `rate`, end in quiet: their last
/// 2 ms, and [`QUIET_FRAMES`] at least, within [`QUIET`]. A sound that does
/// not loop, or loops through a silence, ends so; converted as a sound
/// that does not loop, it keeps its seam.
///
/// The cook restates this rule and the half limits, in nadir-cook's
/// `sound.rs`, to refuse a sound the game could not hold: a change here
/// is a change there.
fn ends_quietly(samples: &[i16], channels: u8, rate: u32) -> bool {
    let frames = (rate as usize / 500).max(QUIET_FRAMES);
    samples
        .iter()
        .rev()
        .take(frames.saturating_mul(usize::from(channels)))
        .all(|sample| sample.unsigned_abs() <= QUIET)
}

/// `samples`, of `channels` at `rate`, converted to [`LOAD_RATE`] as a
/// sound that does not loop, from 8,000 Hz to 384,000; at another rate,
/// as they are.
fn convert(
    path: &str,
    channels: u8,
    rate: u32,
    samples: Vec<f32>,
    limits: Limits,
) -> Result<(u32, Vec<f32>), AssetError> {
    if rate == LOAD_RATE || !(MIN_CONVERTED..=MAX_SAMPLE_RATE).contains(&rate) {
        return Ok((rate, samples));
    }
    let mut converted = pcm::resample(
        &samples,
        usize::from(channels),
        rate,
        LOAD_RATE,
        false,
        limits,
    )
    .map_err(|error| {
        AssetError::Invalid(format!(
            "converting it from {rate} Hz to {LOAD_RATE} Hz: {error}"
        ))
    })?;
    drop(samples);
    // The sinc can carry a peak between two samples past them.
    let mut clipped = 0usize;
    for sample in &mut converted {
        if sample.abs() > 1.0 {
            *sample = sample.clamp(-1.0, 1.0);
            clipped += 1;
        }
    }
    if clipped > 0 {
        warn!(
            "asset `{path}`: {clipped} samples past full scale once converted from {rate} Hz to \
             {LOAD_RATE} Hz, which are clipped: lower its level, or cook it at {LOAD_RATE} Hz"
        );
    }
    Ok((LOAD_RATE, converted))
}

/// A music track, kept compressed, which
/// [`Audio::play_music`](crate::Audio::play_music) plays from its handle,
/// and [`Audio::play_track`](crate::Audio::play_track) as it is.
///
/// As an asset, it loads from a QOA file, `.qoa`, as the cook writes music,
/// of 1 or 2 channels at 1 to 384,000 Hz, every frame of it decoded once on
/// the worker ([`MusicData::new`]); a file larger than the server's limits
/// is refused before it is copied. It keeps its rate: the mixer resamples
/// it as it plays, if it must. A failed track's placeholder is a frame of
/// silence: with placeholders on, playing it fades the music out.
#[derive(Clone, Debug)]
pub struct Music {
    data: Arc<MusicData>,
}

impl Music {
    /// A track of `data`.
    pub fn new(data: MusicData) -> Self {
        Self {
            data: Arc::new(data),
        }
    }

    /// The track. The mixer holds this `Arc` while the track plays, and
    /// [`Audio::update`](crate::Audio::update) drops it.
    pub fn data(&self) -> &Arc<MusicData> {
        &self.data
    }
}

impl Asset for Music {
    type Decoded = MusicData;
    const EXTENSIONS: &'static [&'static str] = &["qoa"];

    fn decode(bytes: &[u8], ctx: &DecodeCtx<'_>) -> Result<MusicData, AssetError> {
        let limit = ctx.limits().max_decoded_bytes;
        if bytes.len() > limit {
            return Err(AssetError::Invalid(format!(
                "it takes {} bytes, more than the limit of {limit}",
                bytes.len()
            )));
        }
        MusicData::new(bytes.to_vec()).map_err(asset_error)
    }

    fn finalize(decoded: MusicData, _ctx: &mut FinalizeCtx<'_>) -> Result<Self, AssetError> {
        Ok(Self::new(decoded))
    }

    fn placeholder(_ctx: &mut FinalizeCtx<'_>) -> Result<Self, AssetError> {
        // A whole frame, so that the stream decodes a frame for every
        // 5,120 samples, not for every one.
        let header = qoa::Header {
            channels: 1,
            sample_rate: LOAD_RATE,
            samples: qoa::FRAME_LEN as u32,
        };
        let bytes = qoa::encode(&header, &[0; qoa::FRAME_LEN]).map_err(AssetError::Decode)?;
        Ok(Self::new(MusicData::new(bytes).map_err(asset_error)?))
    }
}

/// The asset server's error for `error`: a file the codec refuses as it
/// is, anything else as text.
fn asset_error(error: AudioError) -> AssetError {
    match error {
        AudioError::Decode(error) => AssetError::Decode(error),
        error => AssetError::Invalid(error.to_string()),
    }
}
