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

//! Plays beeps through the default audio device, and prints how the stream
//! is doing once a second.
//!
//! ```text
//! cargo run -p nadir-platform --example audio -- [options]
//! ```
//!
//! Options:
//!
//! - `--seconds N`: stop after N seconds; 10 by default.
//! - `--volume V`: how loud the beeps are, from 0 to 1; 0.2 by default.
//! - `--latency MS`: the device's buffer, in milliseconds; 30 by default.
//!
//! Each second, a beep of 440 Hz on the left, then one of 660 Hz on the
//! right.
//!
//! On Linux, `NADIR_ALSA_DEVICE` names the ALSA device to play on in place
//! of `default`, such as `plughw:CARD=Device` (`aplay -L` lists them). A
//! sound server, which `default` usually is, moves the stream to another
//! device when one is unplugged, and the stream never sees it. To see the
//! stream lose a device, aim it at a USB device itself, while the server
//! leaves the device alone, unplug it while the beeps play, and plug it in
//! again: the log says that the stream stopped, with ALSA's reason, then
//! that it is open again, and the beeps go on in time.

use std::error::Error;
use std::f64::consts::TAU;
use std::io::{self, Write};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use nadir_core::cli::Args;
use nadir_platform::{AudioOutputConfig, AudioRender, open_audio};

/// How long a beep lasts, and how long its ends take to ramp, in seconds.
const BEEP: f64 = 0.25;
const RAMP: f64 = 0.005;

fn main() -> ExitCode {
    match run(Args::from_env(), &mut io::stdout()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            // If standard error is closed too, there is nobody left to tell.
            let _ = writeln!(io::stderr(), "audio: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(mut args: Args, out: &mut dyn Write) -> Result<(), Box<dyn Error>> {
    let seconds: f64 = args.parsed("--seconds")?.unwrap_or(10.0);
    let volume: f32 = args.parsed("--volume")?.unwrap_or(0.2);
    let latency_ms: u32 = args.parsed("--latency")?.unwrap_or(30);
    if let Some(extra) = args.finish()?.first() {
        return Err(format!("unexpected argument `{extra}`").into());
    }
    if !(0.0..=1.0).contains(&volume) {
        return Err(format!("the volume {volume} is not from 0 to 1").into());
    }
    let length = Duration::try_from_secs_f64(seconds)?;

    let periods = Arc::new(AtomicU64::new(0));
    let underruns = Arc::new(AtomicU64::new(0));
    let beeps = Beeps {
        frame: 0,
        volume,
        periods: Arc::clone(&periods),
        underruns: Arc::clone(&underruns),
    };
    let config = AudioOutputConfig {
        latency_ms,
        ..AudioOutputConfig::default()
    };
    let stream = open_audio(&config, Box::new(beeps))?;
    writeln!(
        out,
        "audio: {} Hz, {} channels; beeps of 440 Hz on the left, then 660 Hz on the right",
        config.sample_rate, config.channels,
    )?;

    let start = Instant::now();
    let mut second = 1;
    while start.elapsed() < length {
        let next = start + Duration::from_secs(second);
        thread::sleep(next.saturating_duration_since(Instant::now()).min(length));
        let device = match stream.latency() {
            Some(latency) => format!("{:.1} ms of latency", latency.as_secs_f64() * 1000.0),
            None => "no device".to_string(),
        };
        writeln!(
            out,
            "{:6.1} s: {} periods, {} underruns, {device}",
            start.elapsed().as_secs_f64(),
            periods.load(Ordering::Relaxed),
            underruns.load(Ordering::Relaxed),
        )?;
        second += 1;
    }
    drop(stream);
    Ok(())
}

/// A beep on the left in the first quarter of each second, and one on the
/// right in the third.
struct Beeps {
    frame: u64,
    volume: f32,
    periods: Arc<AtomicU64>,
    underruns: Arc<AtomicU64>,
}

impl AudioRender for Beeps {
    fn render(&mut self, out: &mut [f32], channels: u16, sample_rate: u32) {
        let channels = usize::from(channels);
        let rate = f64::from(sample_rate);
        for frame in out.chunks_exact_mut(channels) {
            let time = (self.frame % u64::from(sample_rate)) as f64 / rate;
            let (left, right) = if time < BEEP {
                (beep(time, 440.0), 0.0)
            } else if (0.5..0.5 + BEEP).contains(&time) {
                (0.0, beep(time - 0.5, 660.0))
            } else {
                (0.0, 0.0)
            };
            frame.fill(0.0);
            frame[0] = left * self.volume;
            if let Some(second) = frame.get_mut(1) {
                *second = right * self.volume;
            }
            self.frame += 1;
        }
        self.periods.fetch_add(1, Ordering::Relaxed);
    }

    fn underrun(&mut self) {
        self.underruns.fetch_add(1, Ordering::Relaxed);
    }
}

/// A sine of `frequency` at `time` seconds into a beep, ramped in and out.
fn beep(time: f64, frequency: f64) -> f32 {
    let ramp = (time / RAMP).min((BEEP - time) / RAMP).clamp(0.0, 1.0);
    ((TAU * frequency * time).sin() * ramp) as f32
}
