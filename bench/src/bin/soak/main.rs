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

//! The audio soak: the mixer plays 32 random voices and music through the
//! default device for a number of minutes, and the soak tells of each
//! underrun, the time a callback takes to mix, and the process's memory.
//!
//! Run it as `cargo xtask soak --audio [--minutes N] [--silent]`, which
//! builds it in release mode: 10 minutes by default. `--silent` mixes as
//! ever, and gives the device silence in place of the mix. A line of the
//! numbers so far is printed every 10 s, and each underrun, callback late
//! past the device's buffer, and loss of the device as it comes; then a
//! summary, and `soak: ok`, or `soak: FAILED (...)` and exit status 1.
//!
//! The method is specified in `docs/04-quality.md` ("Soak"), and the
//! choices behind it in ADR-0052.

mod error;
mod load;
mod memory;
mod times;
mod watch;

use std::io::{self, Write};
use std::process::ExitCode;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use nadir_audio::{Audio, AudioConfig};
use nadir_core::CoreError;
use nadir_core::cli::Args;
use nadir_platform::{AudioOutputConfig, open_audio};

use crate::error::SoakError;
use crate::load::{Load, MUSIC_EVERY, VOICES};
use crate::times::{CallbackTimes, Timed};
use crate::watch::{Ending, Watch, plural};

/// The exit code for a command line that was not understood.
const EXIT_USAGE: u8 = 2;

/// The minutes of a soak, unless the command line says: the M6 exit
/// criterion's 10.
const DEFAULT_MINUTES: u32 = 10;

/// The seed of the load's sounds and voices: each run plays the same.
const SEED: u64 = 0x736f_616b;

/// A frame of the main thread, as the game's at 60 Hz.
const FRAME: Duration = Duration::from_nanos(16_666_667);

/// How long, after the stop, the mixer has to hand back what it played:
/// past the longest fade of a loop, 500 ms, and the backend's 2 s wait for
/// a device that takes nothing.
const HAND_BACK: Duration = Duration::from_secs(3);

const USAGE: &str = "Usage: soak --audio [--minutes N] [--silent]\n\n\
    Plays 32 random voices and music through the default audio device for N\n\
    minutes (10), and reports underruns, the mix's time and the memory.\n\
    --silent mixes as ever, but sends the device silence.";

fn main() -> ExitCode {
    let result = run(Args::from_env(), &mut io::stdout());
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        // If standard error is closed too, there is nobody left to tell.
        Err(error @ SoakError::Usage(_)) => {
            let _ = writeln!(io::stderr(), "soak: {error}\n\n{USAGE}");
            ExitCode::from(EXIT_USAGE)
        }
        Err(error) => {
            let _ = writeln!(io::stderr(), "soak: {error}");
            ExitCode::FAILURE
        }
    }
}

/// What the command line asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Options {
    minutes: u32,
    silent: bool,
}

impl Options {
    /// Reads `--audio [--minutes N] [--silent]`.
    fn parse(mut args: Args) -> Result<Self, SoakError> {
        let usage = |error: CoreError| SoakError::Usage(error.to_string());
        let audio = args.flag("--audio");
        let silent = args.flag("--silent");
        let minutes: Option<u32> = args.parsed("--minutes").map_err(usage)?;
        if let Some(argument) = args.finish().map_err(usage)?.first() {
            return Err(SoakError::Usage(format!(
                "the soak takes only options, but got `{argument}`"
            )));
        }
        if !audio {
            return Err(SoakError::Usage(
                "say which soak to run: `--audio`, the only one so far".to_string(),
            ));
        }
        let minutes = minutes.unwrap_or(DEFAULT_MINUTES);
        if minutes == 0 {
            return Err(SoakError::Usage(
                "`--minutes` must be 1 or more".to_string(),
            ));
        }
        Ok(Self { minutes, silent })
    }
}

/// Runs the soak the command line asks for, printing to `out`. Returns
/// whether it passed.
fn run(args: Args, out: &mut dyn Write) -> Result<bool, SoakError> {
    let options = Options::parse(args)?;
    let mut load = Load::new(SEED, MUSIC_EVERY)?;
    let (audio, mixer) = nadir_audio::new(AudioConfig::default())?;
    let times = Arc::new(CallbackTimes::new());
    let render = Timed::new(mixer, Arc::clone(&times), options.silent);
    // The stream plays until it drops, as this function returns.
    let stream = open_audio(&AudioOutputConfig::default(), Box::new(render))?;
    let Some(buffer) = stream.latency() else {
        return Err(SoakError::NoDevice);
    };
    header(out, &options, buffer)?;

    let mut watch = Watch::new(out, &times, memory::read);
    let length = Duration::from_secs(u64::from(options.minutes) * 60);
    let started = Instant::now();
    let mut due = started;
    let mut last = Duration::ZERO;
    let mut now = Duration::ZERO;
    while now < length {
        load.frame(&audio, now, now - last);
        audio.update();
        if !watch.frame(now, &audio.stats(), stream.latency())? {
            break;
        }
        last = now;
        due += FRAME;
        let wake = Instant::now();
        if due > wake {
            thread::sleep(due - wake);
        } else {
            // Behind: the next frame is due a frame from now.
            due = wake;
        }
        now = started.elapsed();
    }
    let ran = now.min(length);
    watch.end(ran, &audio.stats())?;

    let latency = || stream.latency();
    let handed_back = wind_down(&mut load, &audio, &mut watch, &latency, started, HAND_BACK)?;
    let ending = Ending {
        played: load.played(),
        held: load.held(),
        handed_back,
    };
    Ok(watch.summary(options.minutes, ran, &audio.stats(), &ending)?)
}

/// What the soak plays, and on what.
fn header(out: &mut dyn Write, options: &Options, buffer: Duration) -> io::Result<()> {
    let device = match std::env::var("NADIR_ALSA_DEVICE") {
        Ok(device) if !device.is_empty() => format!("the ALSA device `{device}`"),
        _ => "the default device".to_string(),
    };
    let silent = if options.silent {
        ", silent: mixed, but the device gets silence"
    } else {
        ""
    };
    writeln!(
        out,
        "soak: {} of {VOICES} random voices and music, on {device}{silent}",
        plural(u64::from(options.minutes), "minute")
    )?;
    writeln!(
        out,
        "soak: the device's buffer is {:.1} ms; a line every 10 s",
        buffer.as_secs_f64() * 1e3
    )
}

/// Stops the load, and gives the mixer up to `limit` to hand back what it
/// played, updating at each frame, and telling the `watch` of each
/// underrun, late callback and loss of the device, with `latency` the
/// device's and `started` the soak's start. Returns how long it took: at
/// least `limit` if the mixer still holds some, less if the audio thread
/// has stopped.
fn wind_down(
    load: &mut Load,
    audio: &Audio,
    watch: &mut Watch<'_>,
    latency: &dyn Fn() -> Option<Duration>,
    started: Instant,
    limit: Duration,
) -> io::Result<Duration> {
    load.finish(audio);
    let stopped = Instant::now();
    loop {
        audio.update();
        let going = watch.tell(started.elapsed(), &audio.stats(), latency())?;
        let waited = stopped.elapsed();
        if !going || load.held().is_empty() || waited >= limit {
            return Ok(waited);
        }
        thread::sleep(FRAME);
    }
}

#[cfg(test)]
mod tests {
    use nadir_platform::AudioRender;

    use super::*;

    fn options(args: &[&str]) -> Result<Options, String> {
        Options::parse(Args::new(args.iter().copied())).map_err(|error| error.to_string())
    }

    #[test]
    fn the_command_line_takes_the_audio_soak_its_minutes_and_silence() {
        assert_eq!(
            options(&["--audio"]),
            Ok(Options {
                minutes: 10,
                silent: false
            })
        );
        assert_eq!(
            options(&["--minutes", "1", "--silent", "--audio"]),
            Ok(Options {
                minutes: 1,
                silent: true
            })
        );
        assert_eq!(
            options(&["--audio", "--minutes=600"]).map(|options| options.minutes),
            Ok(600)
        );
        assert_eq!(
            options(&[]),
            Err("say which soak to run: `--audio`, the only one so far".to_string())
        );
        assert_eq!(
            options(&["--audio", "--minutes", "0"]),
            Err("`--minutes` must be 1 or more".to_string())
        );
        assert_eq!(
            options(&["--audio", "--minutes", "1.5"]),
            Err("invalid value `1.5` for option `--minutes`".to_string())
        );
        assert_eq!(
            options(&["--audio", "--minutes"]),
            Err("option `--minutes` needs a value".to_string())
        );
        assert_eq!(
            options(&["--audio", "--loud"]),
            Err("unexpected argument `--loud`".to_string())
        );
        assert_eq!(
            options(&["--audio", "ten"]),
            Err("the soak takes only options, but got `ten`".to_string())
        );
    }

    #[test]
    fn the_wind_down_ends_once_everything_is_handed_back() {
        let (audio, mixer) = nadir_audio::new(AudioConfig::default()).unwrap();
        let mut load = Load::new(SEED, MUSIC_EVERY).unwrap();
        load.frame(&audio, Duration::ZERO, Duration::ZERO);
        assert!(!load.held().is_empty());
        // With the mixer gone, the next update drops what it was sent.
        drop(mixer);
        let times = CallbackTimes::new();
        let mut out = Vec::new();
        let mut watch = Watch::new(&mut out, &times, || None);
        let buffer = || Some(Duration::from_millis(30));
        let limit = Duration::from_secs(1);
        let waited = wind_down(
            &mut load,
            &audio,
            &mut watch,
            &buffer,
            Instant::now(),
            limit,
        )
        .unwrap();
        assert!(waited < limit, "{waited:?}");
        assert_eq!(load.held(), []);
    }

    #[test]
    fn the_wind_down_waits_its_limit_for_what_is_held_and_tells_of_a_loss() {
        let (audio, _mixer) = nadir_audio::new(AudioConfig::default()).unwrap();
        let mut load = Load::new(SEED, MUSIC_EVERY).unwrap();
        load.frame(&audio, Duration::ZERO, Duration::ZERO);
        let times = CallbackTimes::new();
        times.record(Duration::from_micros(50), 360, 48_000);
        let mut out = Vec::new();
        let passed = {
            let mut watch = Watch::new(&mut out, &times, || None);
            let none = || None;
            let limit = Duration::from_millis(50);
            // Nothing renders, so the commands hold every sound.
            let waited =
                wind_down(&mut load, &audio, &mut watch, &none, Instant::now(), limit).unwrap();
            assert!(waited >= limit, "{waited:?}");
            let ending = Ending {
                played: load.played(),
                held: load.held(),
                handed_back: waited,
            };
            assert!(!ending.held.is_empty());
            watch
                .summary(1, Duration::from_secs(60), &audio.stats(), &ending)
                .unwrap()
        };
        assert!(!passed);
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text.matches("the device is lost").count(), 1, "{text}");
        assert!(
            text.contains("soak: FAILED (the device lost 1 time, "),
            "{text}"
        );
    }

    /// Renders `render` in periods of 7.5 ms at 48 kHz from `rendered` up
    /// to `until`, as a device would.
    fn play(render: &mut dyn AudioRender, rendered: &mut Duration, until: Duration) {
        const PERIOD: usize = 360;
        let mut samples = [0.0; PERIOD * 2];
        while *rendered < until {
            render.render(&mut samples, 2, 48_000);
            *rendered += Duration::from_secs_f64(PERIOD as f64 / 48_000.0);
        }
    }

    /// The load, the timed mixer and the watch, together as `run` puts
    /// them, but rendered on the test's thread, with a clock of the test's
    /// own: 12 s of the load, with the music crossfading every 4 s, and a
    /// stop.
    #[test]
    fn the_soak_keeps_32_voices_and_the_music_going_and_gets_everything_back() {
        let (audio, mixer) = nadir_audio::new(AudioConfig::default()).unwrap();
        let times = Arc::new(CallbackTimes::new());
        let mut render = Timed::new(mixer, Arc::clone(&times), false);
        let mut load = Load::new(SEED, Duration::from_secs(4)).unwrap();
        // A buffer of an hour, so that the test's own pauses are never
        // late.
        let buffer = Some(Duration::from_secs(3_600));
        let mut out = Vec::new();
        let mut rendered = Duration::ZERO;
        let passed = {
            let mut watch = Watch::new(&mut out, &times, || None);
            let mut fewest = u32::MAX;
            let mut most = 0;
            let mut crossfading = Vec::new();
            let frames: u32 = 12 * 60;
            for frame in 0..frames {
                let now = FRAME * frame;
                load.frame(&audio, now, FRAME);
                audio.update();
                assert!(watch.frame(now, &audio.stats(), buffer).unwrap());
                play(&mut render, &mut rendered, now + FRAME);
                if now >= Duration::from_secs(1) {
                    let voices = audio.stats().active_voices;
                    fewest = fewest.min(voices);
                    most = most.max(voices);
                }
                if frame == 270 {
                    // 4.5 s: track b fades in over track a, from 4 s.
                    crossfading = load.held();
                    render.underrun();
                }
            }
            assert!(
                (28..=32).contains(&fewest) && (32..=40).contains(&most),
                "{fewest} to {most} voices"
            );
            let tracks: Vec<&str> = crossfading
                .iter()
                .map(|(name, _)| *name)
                .filter(|name| name.starts_with("track"))
                .collect();
            assert_eq!(tracks, ["track a", "track b"]);
            let stats = audio.stats();
            assert_eq!(stats.dropped, 0);
            assert_eq!(stats.underruns, 1);
            assert!(load.played() > VOICES as u64, "{} played", load.played());
            watch.end(FRAME * frames, &stats).unwrap();

            load.finish(&audio);
            let stopped = rendered;
            play(
                &mut render,
                &mut rendered,
                stopped + Duration::from_millis(700),
            );
            audio.update();
            let ending = Ending {
                played: load.played(),
                held: load.held(),
                handed_back: rendered - stopped,
            };
            assert_eq!(ending.held, []);
            watch
                .summary(1, Duration::from_secs(60), &audio.stats(), &ending)
                .unwrap()
        };
        assert!(!passed);
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("  0:04  an underrun: 1 so far\n"), "{text}");
        assert!(text.contains("  0:10  callbacks "), "{text}");
        assert!(
            text.contains("  handed back     every sound and track, 0.7 s after the stop\n"),
            "{text}"
        );
        assert!(text.ends_with("soak: FAILED (1 underrun)\n"), "{text}");
    }
}
