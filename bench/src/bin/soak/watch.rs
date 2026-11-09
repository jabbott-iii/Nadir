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

//! What the soak sees as it runs, a frame at a time: the lines it prints as
//! it goes, and the summary and verdict at the end.

use std::io::{self, Write};
use std::time::Duration;

use nadir_audio::AudioStats;

use crate::memory::Memory;
use crate::times::{CallbackTimes, Micros};

/// How often a line of the numbers so far is printed.
pub(crate) const LINE_EVERY: Duration = Duration::from_secs(10);

/// With no callback for this long, the audio thread has stopped: past the
/// backend's 2 s wait for a device that takes nothing, and its tries to
/// open one again.
const STOPPED_AFTER: Duration = Duration::from_secs(5);

/// The voices are counted, and the gaps between callbacks judged, from
/// here: once the first voices have started, and the device plays. (A
/// sound server can take a quarter of a second to start a stream, while
/// the buffer waits full.)
const WARM_UP: Duration = Duration::from_secs(2);

/// What the soak found, for the summary.
pub(crate) struct Ending {
    /// The voices the load started.
    pub(crate) played: u64,
    /// The sounds and tracks the mixer still held after the stop, by
    /// name, with how many times.
    pub(crate) held: Vec<(&'static str, usize)>,
    /// How long after the stop the mixer had handed everything back.
    pub(crate) handed_back: Duration,
}

/// The soak's watch on the audio thread, the device and the process's
/// memory.
pub(crate) struct Watch<'a> {
    out: &'a mut dyn Write,
    times: &'a CallbackTimes,
    memory: fn() -> Option<Memory>,
    next_line: Duration,
    /// The underruns already told of.
    underruns: u64,
    /// The late callbacks, and the longest gap between two, of those
    /// judged: from [`WARM_UP`] after the start, or after the device's
    /// return, while the device plays.
    late: u64,
    longest_gap: Duration,
    judged_from: Duration,
    /// The last frame's time.
    last_frame: Option<Duration>,
    /// The device's buffer, as it was last seen.
    buffer: Option<Duration>,
    /// Since when the device has been lost, while it is, and how many
    /// times it was.
    lost: Option<Duration>,
    losses: u32,
    /// The callbacks counted, and when that count last changed.
    callbacks: (u64, Duration),
    stopped: bool,
    /// The fewest and most voices sounding at a frame: since the last line,
    /// and over the run.
    line_voices: Option<(u32, u32)>,
    run_voices: Option<(u32, u32)>,
    /// The first line's reading of the memory, and the last's, with their
    /// times.
    first_memory: Option<(Duration, Memory)>,
    last_memory: Option<(Duration, Memory)>,
}

impl<'a> Watch<'a> {
    /// Watches what `times` measures, and reads the memory with `memory`,
    /// printing to `out`.
    pub(crate) fn new(
        out: &'a mut dyn Write,
        times: &'a CallbackTimes,
        memory: fn() -> Option<Memory>,
    ) -> Self {
        Self {
            out,
            times,
            memory,
            next_line: LINE_EVERY,
            underruns: 0,
            late: 0,
            longest_gap: Duration::ZERO,
            judged_from: WARM_UP,
            last_frame: None,
            buffer: None,
            lost: None,
            losses: 0,
            callbacks: (0, Duration::ZERO),
            stopped: false,
            line_voices: None,
            run_voices: None,
            first_memory: None,
            last_memory: None,
        }
    }

    /// One frame of the soak, `now` from its start, with the mixer's
    /// `stats` and the device's `latency`: [`Watch::tell`], the voices
    /// sounding, and a line every [`LINE_EVERY`]. Returns `false` once the
    /// audio thread has stopped, when there is nothing left to watch.
    pub(crate) fn frame(
        &mut self,
        now: Duration,
        stats: &AudioStats,
        latency: Option<Duration>,
    ) -> io::Result<bool> {
        if !self.tell(now, stats, latency)? {
            return Ok(false);
        }
        if now >= WARM_UP {
            for voices in [&mut self.line_voices, &mut self.run_voices] {
                widen(voices, stats.active_voices);
            }
        }
        if now >= self.next_line {
            self.line(now, stats)?;
            while self.next_line <= now {
                self.next_line += LINE_EVERY;
            }
        }
        Ok(true)
    }

    /// Tells of each underrun, late callback, and loss and return of the
    /// device, as it comes, and counts them. A callback is judged late only
    /// between two frames from [`WARM_UP`] after the start, or after the
    /// device's return, that both saw the device: the gaps of a loss, and
    /// of a device starting, are its own. Returns `false` once no callback
    /// has come for [`STOPPED_AFTER`]: the audio thread has stopped.
    pub(crate) fn tell(
        &mut self,
        now: Duration,
        stats: &AudioStats,
        latency: Option<Duration>,
    ) -> io::Result<bool> {
        let at = clock(now);
        if let Some(buffer) = latency {
            self.times.set_late_after(buffer);
            self.buffer = Some(buffer);
        }
        match (latency, self.lost) {
            (None, None) => {
                self.lost = Some(now);
                self.losses += 1;
                writeln!(
                    self.out,
                    "  {at}  the device is lost; its periods play on without it"
                )?;
            }
            (Some(_), Some(since)) => {
                self.lost = None;
                self.judged_from = now + WARM_UP;
                let gone = now.saturating_sub(since).as_secs_f64();
                writeln!(self.out, "  {at}  the device is back, after {gone:.1} s")?;
            }
            _ => {}
        }
        if stats.underruns > self.underruns {
            self.underruns = stats.underruns;
            writeln!(self.out, "  {at}  an underrun: {} so far", stats.underruns)?;
        }
        let (late, longest) = self.times.take_gaps();
        let judged =
            latency.is_some() && self.last_frame.is_some_and(|then| then >= self.judged_from);
        self.last_frame = Some(now);
        if judged {
            self.longest_gap = self.longest_gap.max(longest);
            if late > 0 {
                self.late += late;
                writeln!(
                    self.out,
                    "  {at}  a callback late, past the device's buffer: {} so far, the longest gap {}",
                    self.late,
                    millis(self.longest_gap)
                )?;
            }
        }
        let callbacks = self.times.callbacks();
        if callbacks != self.callbacks.0 {
            self.callbacks = (callbacks, now);
        } else if now.saturating_sub(self.callbacks.1) >= STOPPED_AFTER {
            if !self.stopped {
                self.stopped = true;
                writeln!(
                    self.out,
                    "  {at}  no callback for {} s: the audio thread has stopped",
                    STOPPED_AFTER.as_secs()
                )?;
            }
            return Ok(false);
        }
        Ok(true)
    }

    /// Prints the line of the numbers so far, as at the end of the run.
    pub(crate) fn end(&mut self, now: Duration, stats: &AudioStats) -> io::Result<()> {
        self.line(now, stats)
    }

    /// A line of the numbers so far: the callbacks, underruns and late
    /// ones, the 99th percentile and the longest of the mix's time against
    /// the period, the voices sounding since the last line, the commands
    /// dropped, and the memory, with its own part's change since the first
    /// line.
    fn line(&mut self, now: Duration, stats: &AudioStats) -> io::Result<()> {
        let memory = (self.memory)();
        if let Some(memory) = memory {
            self.first_memory.get_or_insert((now, memory));
            self.last_memory = Some((now, memory));
        }
        let voices = match self.line_voices.take() {
            Some((fewest, most)) => format!("{fewest}-{most}"),
            None => "-".to_string(),
        };
        let memory = match (memory, self.first_memory) {
            (Some(memory), Some((_, first))) => format!(
                "{} MiB, own {} MiB ({})",
                mib(memory.resident),
                mib(memory.anonymous),
                change(first.anonymous, memory.anonymous)
            ),
            _ => "not read".to_string(),
        };
        writeln!(
            self.out,
            "  {}  callbacks {}  underruns {}  late {}  mix p99 {}, longest {}, of {}  voices {voices}  \
             dropped {}  memory {memory}",
            clock(now),
            self.times.callbacks(),
            stats.underruns,
            self.late,
            quantile(self.times.quantile(0.99)),
            micros(self.times.longest()),
            self.period()
                .map_or_else(|| "no period yet".to_string(), millis),
            stats.dropped,
        )
    }

    /// How long the last callback's period plays: `None` before the first.
    fn period(&self) -> Option<Duration> {
        let (frames, rate) = self.times.period();
        (rate > 0).then(|| Duration::from_secs_f64(f64::from(frames) / f64::from(rate)))
    }

    /// Prints the summary of a run of `minutes` that went on for `ran`, and
    /// its verdict, and returns whether it passed: with no underrun, no
    /// late callback, no loss of the device, the audio thread going on to
    /// the end, and everything handed back after the stop. Dropped commands
    /// and the memory are told, not judged (ADR-0052).
    pub(crate) fn summary(
        &mut self,
        minutes: u32,
        ran: Duration,
        stats: &AudioStats,
        ending: &Ending,
    ) -> io::Result<bool> {
        let period = self.period();
        let out = &mut *self.out;
        let times = self.times;
        let asked = plural(u64::from(minutes), "minute");
        let length = if ran.as_secs() >= u64::from(minutes) * 60 {
            asked
        } else {
            format!("stopped at {} of {asked}", clock(ran))
        };
        writeln!(out)?;
        writeln!(
            out,
            "soak: {length} of {} voices and music, {} played",
            crate::load::VOICES,
            ending.played
        )?;
        let buffer = self.buffer.map_or_else(|| "none".to_string(), millis);
        writeln!(
            out,
            "  device buffer   {buffer}, a sample's wait there (not its latency from trigger to speaker)"
        )?;
        let (frames, rate) = times.period();
        match period {
            Some(period) => writeln!(
                out,
                "  period          {frames} frames, {} at {rate} Hz",
                millis(period)
            )?,
            None => writeln!(out, "  period          none: no callback")?,
        }
        writeln!(out, "  callbacks       {}", times.callbacks())?;
        writeln!(out, "  underruns       {}", stats.underruns)?;
        writeln!(
            out,
            "  late callbacks  {}, more than the device's buffer after the one before; the longest gap {}",
            self.late,
            millis(self.longest_gap)
        )?;
        let p99 = times.quantile(0.99);
        let share = match (p99, period) {
            (Some(Micros::Under(us)), Some(period)) if !period.is_zero() => format!(
                ": the p99 {:.1}% of the period",
                f64::from(us) / (period.as_secs_f64() * 1e6) * 100.0
            ),
            _ => String::new(),
        };
        writeln!(
            out,
            "  mix time        p50 {}, p99 {}, longest {}{share}",
            quantile(times.quantile(0.5)),
            quantile(p99),
            micros(times.longest())
        )?;
        match self.run_voices {
            Some((fewest, most)) => {
                writeln!(
                    out,
                    "  voices          {fewest} to {most} sounding at a frame"
                )?;
            }
            None => writeln!(out, "  voices          not counted")?,
        }
        writeln!(out, "  dropped         {} commands", stats.dropped)?;
        match (self.first_memory, self.last_memory) {
            (Some((first_at, first)), Some((last_at, last))) => {
                writeln!(
                    out,
                    "  memory          {} MiB resident and {} MiB its own at {}; {} MiB and {} MiB at {} \
                     (own {})",
                    mib(first.resident),
                    mib(first.anonymous),
                    clock(first_at),
                    mib(last.resident),
                    mib(last.anonymous),
                    clock(last_at),
                    change(first.anonymous, last.anonymous)
                )?;
                let peak = if last.peak > first.peak {
                    format!("rose from {} MiB at {}", mib(first.peak), clock(first_at))
                } else {
                    format!("as at {}", clock(first_at))
                };
                writeln!(out, "  peak memory     {} MiB, {peak}", mib(last.peak))?;
            }
            _ => writeln!(out, "  memory          not read on this system")?,
        }
        let handed_back = ending.handed_back.as_secs_f64();
        if ending.held.is_empty() {
            writeln!(
                out,
                "  handed back     every sound and track, {handed_back:.1} s after the stop"
            )?;
        } else {
            let held: Vec<String> = ending
                .held
                .iter()
                .map(|&(name, count)| format!("{name} {}", plural(count as u64, "time")))
                .collect();
            writeln!(
                out,
                "  handed back     not all, {handed_back:.1} s after the stop: the mixer holds {}",
                held.join(", ")
            )?;
        }

        let mut failures = Vec::new();
        if stats.underruns > 0 {
            failures.push(plural(stats.underruns, "underrun"));
        }
        if self.late > 0 {
            failures.push(plural(self.late, "late callback"));
        }
        if self.losses > 0 {
            failures.push(format!(
                "the device lost {}",
                plural(u64::from(self.losses), "time")
            ));
        }
        if self.stopped {
            failures.push("the audio thread stopped".to_string());
        }
        match ending.held.len() {
            0 => {}
            1 => failures.push("1 sound or track not handed back".to_string()),
            held => failures.push(format!("{held} sounds or tracks not handed back")),
        }
        if failures.is_empty() {
            writeln!(out, "soak: ok")?;
        } else {
            writeln!(out, "soak: FAILED ({})", failures.join(", "))?;
        }
        Ok(failures.is_empty())
    }
}

/// Takes `voices` into the fewest and the most.
fn widen(range: &mut Option<(u32, u32)>, voices: u32) {
    let (fewest, most) = range.get_or_insert((voices, voices));
    *fewest = (*fewest).min(voices);
    *most = (*most).max(voices);
}

/// A time from the soak's start, as `m:ss`.
pub(crate) fn clock(time: Duration) -> String {
    let secs = time.as_secs();
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// A duration in milliseconds, to a tenth.
fn millis(duration: Duration) -> String {
    format!("{:.1} ms", duration.as_secs_f64() * 1e3)
}

/// A duration in whole microseconds.
fn micros(duration: Duration) -> String {
    format!("{} µs", duration.as_micros())
}

/// A quantile of the mix's time, as the histogram keeps it.
fn quantile(quantile: Option<Micros>) -> String {
    match quantile {
        None => "-".to_string(),
        Some(Micros::Under(us)) => format!("{us} µs"),
        Some(Micros::Over) => "100 ms or more".to_string(),
    }
}

/// Bytes in mebibytes, to a tenth.
fn mib(bytes: u64) -> String {
    format!("{:.1}", bytes as f64 / f64::from(1 << 20))
}

/// The change from `from` bytes to `to`, in mebibytes, signed.
fn change(from: u64, to: u64) -> String {
    format!("{:+.1} MiB", (to as f64 - from as f64) / f64::from(1 << 20))
}

/// `count` of `thing`, with an "s" past one.
pub(crate) fn plural(count: u64, thing: &str) -> String {
    if count == 1 {
        format!("1 {thing}")
    } else {
        format!("{count} {thing}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIB: u64 = 1 << 20;

    /// A run of a minute, as the summaries are told it went on for.
    const MINUTE: Duration = Duration::from_secs(60);

    const BUFFER: Option<Duration> = Some(Duration::from_millis(30));

    fn memory() -> Option<Memory> {
        Some(Memory {
            resident: 14 * MIB,
            anonymous: 6 * MIB,
            peak: 15 * MIB,
        })
    }

    fn stats(underruns: u64, voices: u32) -> AudioStats {
        AudioStats {
            underruns,
            active_voices: voices,
            ..AudioStats::default()
        }
    }

    fn ending() -> Ending {
        Ending {
            played: 100,
            held: Vec::new(),
            handed_back: Duration::from_millis(300),
        }
    }

    /// Feeds a watch a frame every 100 ms, a callback of 7.5 ms at each,
    /// for `secs` seconds, with what `frame` gives it, and returns what it
    /// printed and its verdict.
    fn watch(
        secs: u64,
        memory: fn() -> Option<Memory>,
        mut frame: impl FnMut(Duration) -> (AudioStats, Option<Duration>),
    ) -> (String, bool) {
        let times = CallbackTimes::new();
        let mut out = Vec::new();
        let mut watch = Watch::new(&mut out, &times, memory);
        let mut last = AudioStats::default();
        for tenth in 0..=secs * 10 {
            let now = Duration::from_millis(tenth * 100);
            times.record(Duration::from_micros(80), 360, 48_000);
            let (stats, latency) = frame(now);
            assert!(watch.frame(now, &stats, latency).unwrap());
            last = stats;
        }
        watch.end(Duration::from_secs(secs), &last).unwrap();
        let passed = watch.summary(1, MINUTE, &last, &ending()).unwrap();
        (String::from_utf8(out).unwrap(), passed)
    }

    #[test]
    fn a_clean_run_prints_a_line_every_10_s_and_passes() {
        let (text, passed) = watch(20, memory, |_| (stats(0, 32), BUFFER));
        assert!(passed, "{text}");
        let lines: Vec<&str> = text.lines().collect();
        assert!(
            lines[0].starts_with("  0:10  callbacks 101  underruns 0  late 0  mix p99 81 µs"),
            "{text}"
        );
        assert!(
            lines[0].contains(
                "of 7.5 ms  voices 32-32  dropped 0  memory 14.0 MiB, own 6.0 MiB (+0.0 MiB)"
            ),
            "{text}"
        );
        assert!(lines[1].starts_with("  0:20  callbacks 201"), "{text}");
        // The end's line, at the same time as the last.
        assert!(lines[2].starts_with("  0:20  callbacks 201"), "{text}");
        assert!(
            text.contains("\nsoak: 1 minute of 32 voices and music, 100 played\n"),
            "{text}"
        );
        assert!(text.contains("  device buffer   30.0 ms,"), "{text}");
        assert!(
            text.contains("  period          360 frames, 7.5 ms at 48000 Hz\n"),
            "{text}"
        );
        assert!(text.contains("  mix time        p50 81 µs, p99 81 µs, longest 80 µs: the p99 1.1% of the period\n"), "{text}");
        assert!(
            text.contains("  voices          32 to 32 sounding at a frame\n"),
            "{text}"
        );
        assert!(
            text.contains("  peak memory     15.0 MiB, as at 0:10\n"),
            "{text}"
        );
        assert!(
            text.contains("  handed back     every sound and track, 0.3 s after the stop\n"),
            "{text}"
        );
        assert!(text.ends_with("soak: ok\n"), "{text}");
    }

    #[test]
    fn each_underrun_and_loss_of_the_device_is_told_as_it_comes_and_fails_the_soak() {
        let (text, passed) = watch(12, memory, |now| {
            let secs = now.as_secs_f64();
            let underruns = if secs >= 3.0 {
                2
            } else {
                u64::from(secs >= 2.0)
            };
            let latency = (!(5.0..7.0).contains(&secs)).then_some(Duration::from_millis(30));
            (stats(underruns, 32), latency)
        });
        assert!(!passed);
        for line in [
            "  0:02  an underrun: 1 so far\n",
            "  0:03  an underrun: 2 so far\n",
            "  0:05  the device is lost; its periods play on without it\n",
            "  0:07  the device is back, after 2.0 s\n",
        ] {
            assert!(text.contains(line), "{line:?} is missing from:\n{text}");
        }
        assert!(
            text.ends_with("soak: FAILED (2 underruns, the device lost 1 time)\n"),
            "{text}"
        );
    }

    #[test]
    fn a_late_callback_and_a_sound_not_handed_back_fail_the_soak() {
        let times = CallbackTimes::new();
        let mut out = Vec::new();
        let mut watch = Watch::new(&mut out, &times, || None);
        let buffer = Some(Duration::from_millis(30));
        // A gap at the start, while the device starts, is not judged.
        times.record(Duration::from_micros(50), 360, 48_000);
        assert!(watch.frame(Duration::ZERO, &stats(0, 0), buffer).unwrap());
        times.gap(Duration::from_millis(250));
        times.record(Duration::from_micros(50), 360, 48_000);
        assert!(
            watch
                .frame(Duration::from_secs(2), &stats(0, 0), buffer)
                .unwrap()
        );
        times.gap(Duration::from_millis(45));
        times.record(Duration::from_micros(50), 360, 48_000);
        let at = Duration::from_millis(2_100);
        assert!(watch.frame(at, &stats(0, 0), buffer).unwrap());
        let ending = Ending {
            held: vec![("hum", 2)],
            ..ending()
        };
        assert!(!watch.summary(1, MINUTE, &stats(0, 0), &ending).unwrap());
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains(
            "  0:02  a callback late, past the device's buffer: 1 so far, the longest gap 45.0 ms\n"
        ), "{text}");
        assert!(
            text.contains("  memory          not read on this system\n"),
            "{text}"
        );
        assert!(
            text.contains("  voices          0 to 0 sounding at a frame\n"),
            "{text}"
        );
        assert!(text.contains("the mixer holds hum 2 times\n"), "{text}");
        assert!(
            text.ends_with("soak: FAILED (1 late callback, 1 sound or track not handed back)\n"),
            "{text}"
        );
    }

    #[test]
    fn no_callback_for_5_s_ends_the_watch() {
        let times = CallbackTimes::new();
        let mut out = Vec::new();
        let mut watch = Watch::new(&mut out, &times, || None);
        times.record(Duration::from_micros(50), 360, 48_000);
        let buffer = Some(Duration::from_millis(30));
        assert!(
            watch
                .frame(Duration::from_secs(1), &stats(0, 0), buffer)
                .unwrap()
        );
        assert!(
            watch
                .frame(Duration::from_millis(5_999), &stats(0, 0), buffer)
                .unwrap()
        );
        assert!(
            !watch
                .frame(Duration::from_secs(6), &stats(0, 0), None)
                .unwrap()
        );
        let ran = Duration::from_secs(6);
        assert!(!watch.summary(1, ran, &stats(0, 0), &ending()).unwrap());
        // Told again, it says the same once.
        assert!(
            !watch
                .tell(Duration::from_secs(7), &stats(0, 0), None)
                .unwrap()
        );
        let text = String::from_utf8(out).unwrap();
        assert!(text.starts_with("  0:06  the device is lost"), "{text}");
        assert_eq!(text.matches("no callback for 5 s").count(), 1, "{text}");
        assert!(
            text.contains(
                "\nsoak: stopped at 0:06 of 1 minute of 32 voices and music, 100 played\n"
            ),
            "{text}"
        );
        assert!(
            text.contains("  0:06  no callback for 5 s: the audio thread has stopped\n"),
            "{text}"
        );
        assert!(
            text.ends_with("soak: FAILED (the device lost 1 time, the audio thread stopped)\n"),
            "{text}"
        );
    }

    #[test]
    fn the_voices_are_counted_from_the_warm_up_and_each_line_has_its_own() {
        let (text, passed) = watch(20, memory, |now| {
            // None yet at the start, then 30 to 33 up to 10 s, and 31 to 34
            // after.
            let tenths = now.as_millis() / 100;
            let voices = match tenths {
                0..=9 => 0,
                10..=100 => 30 + (tenths % 4) as u32,
                _ => 31 + (tenths % 4) as u32,
            };
            (stats(0, voices), BUFFER)
        });
        assert!(passed, "{text}");
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines[0].contains("  voices 30-33  "), "{text}");
        assert!(lines[1].contains("  voices 31-34  "), "{text}");
        // The end's line comes with no frame since the last.
        assert!(lines[2].contains("  voices -  "), "{text}");
        assert!(
            text.contains("  voices          30 to 34 sounding at a frame\n"),
            "{text}"
        );
    }

    #[test]
    fn dropped_commands_are_told_not_judged() {
        let dropped = AudioStats {
            dropped: 5,
            active_voices: 32,
            ..AudioStats::default()
        };
        let (text, passed) = watch(10, memory, |_| (dropped, BUFFER));
        assert!(passed, "{text}");
        assert!(text.contains("  dropped 5  "), "{text}");
        assert!(text.contains("  dropped         5 commands\n"), "{text}");
    }

    #[test]
    fn an_underrun_is_told_once() {
        let (text, _) = watch(5, memory, |now| {
            (stats(u64::from(now >= Duration::from_secs(2)), 32), BUFFER)
        });
        assert_eq!(text.matches("an underrun").count(), 1, "{text}");
        assert!(text.ends_with("soak: FAILED (1 underrun)\n"), "{text}");
    }

    #[test]
    fn a_sound_held_once_is_not_handed_back() {
        let times = CallbackTimes::new();
        let mut out = Vec::new();
        let mut watch = Watch::new(&mut out, &times, || None);
        let once = Ending {
            held: vec![("hum", 1)],
            ..ending()
        };
        assert!(!watch.summary(1, MINUTE, &stats(0, 0), &once).unwrap());
        let two = Ending {
            held: vec![("hum", 1), ("track a", 3)],
            ..ending()
        };
        assert!(!watch.summary(1, MINUTE, &stats(0, 0), &two).unwrap());
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("the mixer holds hum 1 time\n"), "{text}");
        assert!(
            text.contains("soak: FAILED (1 sound or track not handed back)\n"),
            "{text}"
        );
        assert!(
            text.contains("the mixer holds hum 1 time, track a 3 times\n"),
            "{text}"
        );
        assert!(
            text.ends_with("soak: FAILED (2 sounds or tracks not handed back)\n"),
            "{text}"
        );
    }

    #[test]
    fn a_stalled_main_thread_prints_one_line_and_goes_on_every_10_s() {
        let times = CallbackTimes::new();
        let mut out = Vec::new();
        let mut watch = Watch::new(&mut out, &times, || None);
        for at in [4_900, 35_000, 35_100, 39_900, 40_000] {
            times.record(Duration::from_micros(50), 360, 48_000);
            let now = Duration::from_millis(at);
            assert!(watch.frame(now, &stats(0, 32), BUFFER).unwrap());
        }
        let text = String::from_utf8(out).unwrap();
        let lines: Vec<&str> = text.lines().map(|line| &line[..7]).collect();
        assert_eq!(lines, ["  0:35 ", "  0:40 "], "{text}");
    }

    #[test]
    fn the_gaps_of_a_loss_and_of_the_device_s_return_are_not_late() {
        let times = CallbackTimes::new();
        let mut out = Vec::new();
        let mut watch = Watch::new(&mut out, &times, || None);
        let mut frame = |ms: u64, latency: Option<Duration>, gap: Option<u64>| {
            if let Some(gap) = gap {
                times.gap(Duration::from_millis(gap));
            }
            times.record(Duration::from_micros(50), 360, 48_000);
            let now = Duration::from_millis(ms);
            assert!(watch.frame(now, &stats(0, 32), latency).unwrap());
        };
        frame(3_000, BUFFER, None);
        // A stall of 2 s, then the loss.
        frame(5_000, None, Some(2_000));
        frame(5_100, None, Some(900));
        // The device is back, and starts slowly: not judged for 2 s.
        frame(7_000, BUFFER, Some(250));
        frame(8_900, BUFFER, Some(40));
        frame(9_100, BUFFER, Some(8));
        // Judged again.
        frame(9_200, BUFFER, Some(45));
        assert!(!watch.summary(1, MINUTE, &stats(0, 32), &ending()).unwrap());
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text.matches("a callback late").count(), 1, "{text}");
        assert!(text.contains(
            "  0:09  a callback late, past the device's buffer: 1 so far, the longest gap 45.0 ms\n"
        ), "{text}");
        assert!(text.contains("  late callbacks  1, "), "{text}");
        assert!(text.contains("the longest gap 45.0 ms\n"), "{text}");
        assert!(
            text.ends_with("soak: FAILED (1 late callback, the device lost 1 time)\n"),
            "{text}"
        );
    }

    #[test]
    fn the_memory_s_growth_is_its_own_part_s_and_a_rising_peak_is_told() {
        fn growing() -> Option<Memory> {
            use std::sync::atomic::{AtomicU64, Ordering};
            static READS: AtomicU64 = AtomicU64::new(0);
            let reads = READS.fetch_add(1, Ordering::Relaxed);
            Some(Memory {
                resident: 20 * MIB,
                anonymous: 6 * MIB + reads * MIB / 2,
                peak: 20 * MIB + reads * MIB,
            })
        }
        let (text, passed) = watch(30, growing, |_| {
            (stats(0, 32), Some(Duration::from_millis(30)))
        });
        assert!(passed, "memory is told, not judged:\n{text}");
        assert!(text.contains("  0:20  callbacks"), "{text}");
        assert!(text.contains("own 6.5 MiB (+0.5 MiB)\n"), "{text}");
        assert!(text.contains(
            "  memory          20.0 MiB resident and 6.0 MiB its own at 0:10; 20.0 MiB and 7.5 MiB at 0:30 (own +1.5 MiB)\n"
        ), "{text}");
        assert!(
            text.contains("  peak memory     23.0 MiB, rose from 20.0 MiB at 0:10\n"),
            "{text}"
        );
    }

    #[test]
    fn times_and_sizes_read_as_people_write_them() {
        assert_eq!(clock(Duration::from_millis(59_999)), "0:59");
        assert_eq!(clock(Duration::from_secs(600)), "10:00");
        assert_eq!(clock(Duration::from_secs(7_265)), "121:05");
        assert_eq!(millis(Duration::from_micros(7_500)), "7.5 ms");
        assert_eq!(quantile(None), "-");
        assert_eq!(quantile(Some(Micros::Over)), "100 ms or more");
        assert_eq!(mib(3 * MIB / 2), "1.5");
        assert_eq!(change(2 * MIB, MIB), "-1.0 MiB");
        assert_eq!(plural(0, "underrun"), "0 underruns");
        assert_eq!(plural(1, "underrun"), "1 underrun");
    }
}
