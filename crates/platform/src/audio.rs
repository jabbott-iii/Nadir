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

//! Audio output: a stream to the default device, which an audio thread of
//! its own fills through a callback.

use std::time::Duration;

use crate::PlatformError;
#[cfg(target_os = "linux")]
use crate::linux::alsa as backend;
#[cfg(not(target_os = "linux"))]
use crate::no_audio as backend;

/// What audio output to open, as [`open_audio`] takes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioOutputConfig {
    /// The frames a second, from 8,000 to 384,000: 48,000 by default.
    pub sample_rate: u32,
    /// The samples of a frame, from 1 to 8: 2 by default, left then right.
    pub channels: u16,
    /// About how long a sample waits in the device's buffer before it is
    /// heard, in milliseconds, from 5 to 500: 30 by default. A longer one
    /// rides out a busy moment of the audio thread; a shorter one makes a
    /// sound start sooner.
    pub latency_ms: u32,
}

impl Default for AudioOutputConfig {
    /// 48 kHz stereo, with 30 ms of latency.
    fn default() -> Self {
        Self {
            sample_rate: 48_000,
            channels: 2,
            latency_ms: 30,
        }
    }
}

impl AudioOutputConfig {
    /// Checks each field against its range.
    fn validate(&self) -> Result<(), PlatformError> {
        let check = |ok: bool, field, expected| {
            if ok {
                Ok(())
            } else {
                Err(PlatformError::InvalidAudioConfig { field, expected })
            }
        };
        check((1..=8).contains(&self.channels), "channels", "1 to 8")?;
        check(
            (8_000..=384_000).contains(&self.sample_rate),
            "sample_rate",
            "8,000 to 384,000",
        )?;
        check(
            (5..=500).contains(&self.latency_ms),
            "latency_ms",
            "5 to 500",
        )
    }
}

/// What makes the sound: the audio thread calls it, as [`open_audio`]
/// describes.
///
/// It runs on the audio thread, which the device waits on: it must not
/// block, allocate, free or log. A mixer that the main thread talks to
/// through a queue it never waits on, as `nadir-audio`'s, fits. A render
/// that panics ends the stream: the device is closed, nothing more plays,
/// and the panic hook reports it.
pub trait AudioRender: Send + 'static {
    /// Overwrites every sample of `out` with `out.len() / channels`
    /// interleaved frames of `channels` samples, from −1 to 1, at
    /// `sample_rate` frames a second, which follow those of the call
    /// before. The length can change from one call to the next; on Linux
    /// it is one period of the device, until the device is opened again.
    fn render(&mut self, out: &mut [f32], channels: u16, sample_rate: u32);

    /// The device ran out of samples (an underrun), so that a gap was
    /// heard. Called on the audio thread, once for each the system reports,
    /// before the next [`render`](AudioRender::render).
    fn underrun(&mut self);
}

/// Audio output that is playing: an audio thread, and the device it fills.
///
/// Dropping it stops the thread and closes the device; what the device had
/// not played yet is dropped. The drop waits for the thread up to 2 s,
/// which it takes only when the system holds the device, as while it
/// resumes from a suspend; past that, the thread is left to end with the
/// process.
///
/// It is `Send` and `Sync` on every system.
pub struct AudioStream {
    inner: backend::Stream,
}

// The same on every system (AGENTS.md, rule 4).
const _: fn() = || {
    fn send_and_sync<T: Send + Sync>() {}
    send_and_sync::<AudioStream>();
};

impl AudioStream {
    /// How long a sample waits in the device's buffer before it is heard,
    /// as the device took it the last time it opened, besides what a sound
    /// server adds. `None` while there is no device: before the first one
    /// opens, and from the loss of one until it opens again.
    pub fn latency(&self) -> Option<Duration> {
        self.inner.latency()
    }
}

/// Opens the default audio output device, and starts an audio thread that
/// keeps it fed from `render`.
///
/// It returns once the thread has opened the device, or found that there
/// is none, or after 2 s, when the device is slow to open, as a sound
/// server that does not answer makes it: the stream then starts without
/// it. Until the stream is dropped, the thread calls
/// [`AudioRender::render`] for a period of the device at a time, and
/// writes it as room comes, so that what plays keeps time with the device;
/// a device with no clock of its own, which takes samples as fast as they
/// come, as ALSA's `null`, is held to the system's clock. Each underrun is
/// [`AudioRender::underrun`], and the device is recovered.
///
/// If there is no device, or it stops working, as when it is unplugged, or
/// takes no samples for 2 s, the thread logs a warning, and goes on
/// calling `render` in time while it tries to open the device again once a
/// second. It logs again when it has, or when a device opens but refuses
/// the format. It logs nothing else.
///
/// # Platforms
///
/// - **Linux:** ALSA's `default` device, which a sound server, PipeWire or
///   PulseAudio, usually provides, or the ALSA device that
///   `NADIR_ALSA_DEVICE` names, such as `plughw:CARD=Device`, when it is set
///   and not empty. `libasound.so.2` is loaded when audio is first opened,
///   not linked. The device takes the rate and channels asked for, as ALSA
///   converts and resamples where it must, or the call fails.
/// - **Windows and macOS:** not built yet (roadmap tasks M6.4 and M6.5):
///   this returns [`PlatformError::Unsupported`].
///
/// # Errors
///
/// - [`PlatformError::InvalidAudioConfig`] for a field of `config` out of
///   its range, on every system.
/// - [`PlatformError::LibraryNotFound`] or [`PlatformError::MissingSymbols`]
///   if the system's audio library is missing or too old.
/// - [`PlatformError::AudioCallFailed`] if the device opens but does not
///   take `config`, within the 2 s, or the thread cannot be started. A
///   device that does not open is no error: the stream starts without one,
///   as above.
///
/// # Examples
///
/// A second of a quiet 440 Hz tone:
///
/// ```no_run
/// use std::f32::consts::TAU;
/// use std::thread;
/// use std::time::Duration;
///
/// use nadir_platform::{AudioOutputConfig, AudioRender, open_audio};
///
/// struct Tone {
///     phase: f32,
/// }
///
/// impl AudioRender for Tone {
///     fn render(&mut self, out: &mut [f32], channels: u16, sample_rate: u32) {
///         let step = 440.0 / sample_rate as f32;
///         for frame in out.chunks_exact_mut(usize::from(channels)) {
///             frame.fill((self.phase * TAU).sin() * 0.1);
///             self.phase = (self.phase + step).fract();
///         }
///     }
///
///     fn underrun(&mut self) {}
/// }
///
/// let config = AudioOutputConfig::default();
/// let stream = open_audio(&config, Box::new(Tone { phase: 0.0 }))?;
/// thread::sleep(Duration::from_secs(1));
/// drop(stream);
/// # Ok::<(), nadir_platform::PlatformError>(())
/// ```
pub fn open_audio(
    config: &AudioOutputConfig,
    render: Box<dyn AudioRender>,
) -> Result<AudioStream, PlatformError> {
    config.validate()?;
    let inner = backend::open(config, render)?;
    Ok(AudioStream { inner })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A render that is never called.
    struct Never;

    impl AudioRender for Never {
        fn render(&mut self, _out: &mut [f32], _channels: u16, _sample_rate: u32) {
            unreachable!("the stream does not start");
        }

        fn underrun(&mut self) {
            unreachable!("the stream does not start");
        }
    }

    fn refused(config: AudioOutputConfig) -> (&'static str, &'static str) {
        match open_audio(&config, Box::new(Never)) {
            Err(PlatformError::InvalidAudioConfig { field, expected }) => (field, expected),
            Err(error) => panic!("{error}"),
            Ok(_) => panic!("the stream started"),
        }
    }

    #[test]
    fn each_field_out_of_its_range_is_refused_on_every_system() {
        let config = AudioOutputConfig::default();
        for channels in [0, 9, u16::MAX] {
            let field = refused(AudioOutputConfig { channels, ..config });
            assert_eq!(field, ("channels", "1 to 8"), "{channels} channels");
        }
        for sample_rate in [0, 7_999, 384_001, u32::MAX] {
            let field = refused(AudioOutputConfig {
                sample_rate,
                ..config
            });
            assert_eq!(
                field,
                ("sample_rate", "8,000 to 384,000"),
                "{sample_rate} Hz"
            );
        }
        for latency_ms in [0, 4, 501, u32::MAX] {
            let field = refused(AudioOutputConfig {
                latency_ms,
                ..config
            });
            assert_eq!(field, ("latency_ms", "5 to 500"), "{latency_ms} ms");
        }
    }

    #[test]
    fn each_range_includes_its_ends() {
        for config in [
            AudioOutputConfig {
                sample_rate: 8_000,
                channels: 1,
                latency_ms: 5,
            },
            AudioOutputConfig {
                sample_rate: 384_000,
                channels: 8,
                latency_ms: 500,
            },
        ] {
            assert_eq!(config.validate(), Ok(()), "{config:?}");
        }
        assert_eq!(AudioOutputConfig::default().validate(), Ok(()));
    }
}
