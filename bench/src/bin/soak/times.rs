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

//! What the audio thread measures of each callback, for the main thread to
//! read: how long its mix took, kept in a histogram, and how long it came
//! after the one before.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use nadir_audio::Mixer;
use nadir_platform::AudioRender;

/// The histogram's buckets of 1 µs, from 0 to 99,999 µs. One more holds
/// the callbacks of 100 ms and more.
pub(crate) const BUCKETS: usize = 100_000;

/// A callback's time, to the microsecond above, as the histogram keeps it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Micros {
    /// Less than this many microseconds, and at least one fewer.
    Under(u32),
    /// 100 ms or more.
    Over,
}

/// What the audio thread measures, shared with the main thread. All its
/// memory is allocated when it is made: the audio thread only adds to it,
/// with atomics, and never waits.
///
/// A bucket counts up to `u32::MAX` callbacks, 49 days of 1,000 a second.
pub(crate) struct CallbackTimes {
    /// The callbacks by how long their mix took, a microsecond a bucket.
    buckets: Box<[AtomicU32]>,
    callbacks: AtomicU64,
    longest_ns: AtomicU64,
    /// The frames and the rate of the last callback.
    frames: AtomicU32,
    rate: AtomicU32,
    /// A callback that starts more than this after the one before it is
    /// late: the device's buffer, which the main thread keeps up to date.
    /// At 0, before the first frame, no gap is judged, nor kept as the
    /// longest.
    late_after_ns: AtomicU64,
    late: AtomicU64,
    longest_gap_ns: AtomicU64,
}

impl CallbackTimes {
    pub(crate) fn new() -> Self {
        Self {
            buckets: (0..=BUCKETS).map(|_| AtomicU32::new(0)).collect(),
            callbacks: AtomicU64::new(0),
            longest_ns: AtomicU64::new(0),
            frames: AtomicU32::new(0),
            rate: AtomicU32::new(0),
            late_after_ns: AtomicU64::new(0),
            late: AtomicU64::new(0),
            longest_gap_ns: AtomicU64::new(0),
        }
    }

    /// Counts a callback whose mix took `took`, of `frames` frames at
    /// `rate`: on the audio thread.
    pub(crate) fn record(&self, took: Duration, frames: usize, rate: u32) {
        let ns = nanos(took);
        let bucket = usize::try_from(ns / 1_000).map_or(BUCKETS, |us| us.min(BUCKETS));
        self.buckets[bucket].fetch_add(1, Ordering::Relaxed);
        self.callbacks.fetch_add(1, Ordering::Relaxed);
        self.longest_ns.fetch_max(ns, Ordering::Relaxed);
        self.frames
            .store(u32::try_from(frames).unwrap_or(u32::MAX), Ordering::Relaxed);
        self.rate.store(rate, Ordering::Relaxed);
    }

    /// Counts the time from one callback's start to the next one's, once
    /// there is a buffer to judge it by: on the audio thread.
    pub(crate) fn gap(&self, gap: Duration) {
        let late_after = self.late_after_ns.load(Ordering::Relaxed);
        if late_after == 0 {
            return;
        }
        let ns = nanos(gap);
        self.longest_gap_ns.fetch_max(ns, Ordering::Relaxed);
        if ns > late_after {
            self.late.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Makes a callback late that starts more than `buffer` after the one
    /// before it.
    pub(crate) fn set_late_after(&self, buffer: Duration) {
        self.late_after_ns.store(nanos(buffer), Ordering::Relaxed);
    }

    /// The callbacks counted.
    pub(crate) fn callbacks(&self) -> u64 {
        self.callbacks.load(Ordering::Relaxed)
    }

    /// The longest a mix took.
    pub(crate) fn longest(&self) -> Duration {
        Duration::from_nanos(self.longest_ns.load(Ordering::Relaxed))
    }

    /// The callbacks that started late, and the longest from one
    /// callback's start to the next one's, since the last call: the main
    /// thread judges whether they count.
    pub(crate) fn take_gaps(&self) -> (u64, Duration) {
        let late = self.late.swap(0, Ordering::Relaxed);
        let longest = self.longest_gap_ns.swap(0, Ordering::Relaxed);
        (late, Duration::from_nanos(longest))
    }

    /// The last callback's frames and rate: 0 and 0 before the first.
    pub(crate) fn period(&self) -> (u32, u32) {
        (
            self.frames.load(Ordering::Relaxed),
            self.rate.load(Ordering::Relaxed),
        )
    }

    /// The least time that a share `q`, from 0 to 1, of the callbacks took
    /// less than, to the microsecond above: `q` 0.99 gives the 99th
    /// percentile. `None` before the first callback.
    pub(crate) fn quantile(&self, q: f64) -> Option<Micros> {
        let counts = || {
            self.buckets
                .iter()
                .map(|count| count.load(Ordering::Relaxed))
        };
        let total: u64 = counts().map(u64::from).sum();
        if total == 0 {
            return None;
        }
        // The rank of the callback that the share reaches, from 1.
        let rank = ((q * total as f64).ceil() as u64).clamp(1, total);
        let mut seen = 0;
        for (bucket, count) in counts().enumerate() {
            seen += u64::from(count);
            if seen >= rank {
                return Some(match u32::try_from(bucket) {
                    Ok(us) if bucket < BUCKETS => Micros::Under(us + 1),
                    _ => Micros::Over,
                });
            }
        }
        // The callbacks counted since the sum are past the rank's.
        Some(Micros::Over)
    }
}

/// A duration in nanoseconds, up to `u64::MAX`.
fn nanos(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

/// The mixer, as the audio thread renders it, timed: the facade's
/// `MixerOutput`, besides what it measures.
pub(crate) struct Timed {
    mixer: Mixer,
    times: Arc<CallbackTimes>,
    /// Whether the device gets silence once the mix is done, in place of
    /// it.
    silent: bool,
    /// When the last callback started.
    last: Option<Instant>,
}

impl Timed {
    pub(crate) fn new(mixer: Mixer, times: Arc<CallbackTimes>, silent: bool) -> Self {
        Self {
            mixer,
            times,
            silent,
            last: None,
        }
    }
}

impl AudioRender for Timed {
    fn render(&mut self, out: &mut [f32], channels: u16, sample_rate: u32) {
        let started = Instant::now();
        if let Some(last) = self.last {
            self.times.gap(started.saturating_duration_since(last));
        }
        self.last = Some(started);
        self.mixer.render(out, usize::from(channels), sample_rate);
        let took = started.elapsed();
        if self.silent {
            out.fill(0.0);
        }
        let frames = out.len() / usize::from(channels.max(1));
        self.times.record(took, frames, sample_rate);
    }

    fn underrun(&mut self) {
        self.mixer.count_underrun();
    }
}

#[cfg(test)]
mod tests {
    use std::thread;

    use nadir_audio::{AudioConfig, PlayParams, Sound, SoundData};

    use super::*;

    fn micros(us: u64) -> Duration {
        Duration::from_micros(us)
    }

    #[test]
    fn quantiles_are_the_microsecond_above_the_callback_they_reach() {
        let times = CallbackTimes::new();
        assert_eq!(times.quantile(0.99), None);
        // 98 callbacks of 40.5 µs, one of 83.9 µs and one of 250 µs.
        for _ in 0..98 {
            times.record(Duration::from_nanos(40_500), 360, 48_000);
        }
        times.record(Duration::from_nanos(83_900), 360, 48_000);
        times.record(micros(250), 512, 44_100);
        assert_eq!(times.callbacks(), 100);
        assert_eq!(times.quantile(0.5), Some(Micros::Under(41)));
        assert_eq!(times.quantile(0.98), Some(Micros::Under(41)));
        // The 99th of the 100.
        assert_eq!(times.quantile(0.99), Some(Micros::Under(84)));
        assert_eq!(times.quantile(1.0), Some(Micros::Under(251)));
        // A share of 0 still reaches the first callback.
        assert_eq!(times.quantile(0.0), Some(Micros::Under(41)));
        assert_eq!(times.longest(), micros(250));
        assert_eq!(times.period(), (512, 44_100));
    }

    #[test]
    fn the_rank_a_share_reaches_is_rounded_up() {
        // 99% of 2,667 callbacks is 2,640.33 of them: the 2,641st, the
        // first of the slower ones, is the 99th percentile.
        let times = CallbackTimes::new();
        for _ in 0..2_640 {
            times.record(micros(10), 360, 48_000);
        }
        for _ in 0..27 {
            times.record(micros(20), 360, 48_000);
        }
        assert_eq!(times.quantile(0.99), Some(Micros::Under(21)));
        assert_eq!(times.quantile(0.98), Some(Micros::Under(11)));
    }

    #[test]
    fn a_whole_microsecond_falls_in_the_bucket_above_it() {
        let times = CallbackTimes::new();
        times.record(micros(84), 1, 48_000);
        assert_eq!(times.quantile(0.99), Some(Micros::Under(85)));
        let times = CallbackTimes::new();
        times.record(Duration::ZERO, 1, 48_000);
        assert_eq!(times.quantile(0.99), Some(Micros::Under(1)));
    }

    #[test]
    fn callbacks_of_100_ms_and_more_share_the_last_bucket() {
        let times = CallbackTimes::new();
        times.record(Duration::from_nanos(99_999_999), 1, 48_000);
        assert_eq!(times.quantile(1.0), Some(Micros::Under(100_000)));
        times.record(Duration::from_millis(100), 1, 48_000);
        times.record(Duration::from_secs(u64::MAX), 1, 48_000);
        assert_eq!(times.quantile(0.5), Some(Micros::Over));
        assert_eq!(times.quantile(1.0), Some(Micros::Over));
        assert_eq!(times.longest(), Duration::from_nanos(u64::MAX));
    }

    #[test]
    fn a_gap_longer_than_the_buffer_is_late() {
        let times = CallbackTimes::new();
        // Before the buffer is known, nothing is late, nor the longest.
        times.gap(Duration::from_secs(1));
        assert_eq!(times.take_gaps(), (0, Duration::ZERO));
        times.set_late_after(Duration::from_millis(30));
        times.gap(Duration::from_micros(7_500));
        times.gap(Duration::from_millis(30));
        assert_eq!(times.take_gaps(), (0, Duration::from_millis(30)));
        times.gap(Duration::from_nanos(30_000_001));
        times.gap(Duration::from_millis(40));
        times.gap(Duration::from_micros(7_500));
        assert_eq!(times.take_gaps(), (2, Duration::from_millis(40)));
        // Each call takes what came since the last.
        assert_eq!(times.take_gaps(), (0, Duration::ZERO));
    }

    #[test]
    fn callbacks_counted_on_many_threads_all_count() {
        let times = Arc::new(CallbackTimes::new());
        let threads: Vec<_> = (0..4)
            .map(|thread| {
                let times = Arc::clone(&times);
                thread::spawn(move || {
                    for us in 0..1_000 {
                        times.record(micros(thread * 1_000 + us), 1, 48_000);
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        assert_eq!(times.callbacks(), 4_000);
        assert_eq!(times.quantile(1.0), Some(Micros::Under(4_000)));
        assert_eq!(times.quantile(0.5), Some(Micros::Under(2_000)));
    }

    #[test]
    fn the_render_times_the_mix_and_counts_underruns_and_gaps() {
        let (audio, mixer) = nadir_audio::new(AudioConfig::default()).unwrap();
        let times = Arc::new(CallbackTimes::new());
        let mut render: Box<dyn AudioRender> =
            Box::new(Timed::new(mixer, Arc::clone(&times), false));
        let tone: Vec<f32> = (0..4_800)
            .map(|i| ((i % 100) as f32 / 100.0) - 0.5)
            .collect();
        let sound = Sound::new(SoundData::new(1, 48_000, tone).unwrap());
        audio.play_sound(&sound, PlayParams::default());

        let mut out = vec![0.0; 360 * 2];
        render.render(&mut out, 2, 48_000);
        assert!(out.iter().any(|&sample| sample != 0.0), "the mix is heard");
        render.underrun();
        times.set_late_after(Duration::from_millis(5));
        thread::sleep(Duration::from_millis(10));
        render.render(&mut out, 2, 48_000);

        assert_eq!(times.callbacks(), 2);
        assert_eq!(times.period(), (360, 48_000));
        let (late, longest) = times.take_gaps();
        assert_eq!(late, 1);
        assert!(longest >= Duration::from_millis(10));
        assert_eq!(audio.stats().underruns, 1);
    }

    #[test]
    fn a_silent_render_mixes_but_sends_silence() {
        let (audio, mixer) = nadir_audio::new(AudioConfig::default()).unwrap();
        let times = Arc::new(CallbackTimes::new());
        let mut render = Timed::new(mixer, Arc::clone(&times), true);
        let tone: Vec<f32> = (0..4_800)
            .map(|i| ((i % 100) as f32 / 100.0) - 0.5)
            .collect();
        let sound = Sound::new(SoundData::new(1, 48_000, tone).unwrap());
        audio.play_sound(&sound, PlayParams::default());

        let mut out = vec![1.0; 360 * 2];
        render.render(&mut out, 2, 48_000);
        assert!(out.iter().all(|&sample| sample == 0.0));
        assert_eq!(audio.stats().active_voices, 1, "the voice was mixed");
        assert_eq!(times.callbacks(), 1);
        // A device of one channel: its period is as many frames as samples.
        let mut out = vec![1.0; 441];
        render.render(&mut out, 1, 44_100);
        assert!(out.iter().all(|&sample| sample == 0.0));
        assert_eq!(times.period(), (441, 44_100));
    }
}
