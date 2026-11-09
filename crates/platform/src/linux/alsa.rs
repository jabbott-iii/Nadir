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

//! Audio output on Linux: ALSA's `default` device, or the one
//! `NADIR_ALSA_DEVICE` names, through `libasound.so.2`, which is loaded the
//! first time audio is opened.
//!
//! An audio thread of the stream's own opens the device with
//! `SND_PCM_NONBLOCK`: a device that another program holds fails at once,
//! with `-EBUSY`, and a full buffer gives `-EAGAIN`. A plugin's open, as of
//! a sound server that does not answer, can still wait: [`open`] waits for
//! it [`OPEN_WAIT`] at most, then goes on as if there were no device. The
//! thread renders a period at a time, and writes it as room comes, waiting
//! for room with `snd_pcm_wait` a slice at a time, so that it sees a stop
//! within a slice; a device that never makes it wait, as ALSA's `null`, is
//! held to the clock. An underrun or a suspend is recovered with
//! `snd_pcm_recover`. Any other failure, or a device that takes no frames
//! for [`STALL`], loses the device: the thread closes it, logs why, and
//! goes on rendering in time while it tries to open it again every
//! [`RETRY`]. A device that does not open at the start is treated the same
//! way.

use core::ffi::{CStr, c_char, c_int, c_uint, c_void};
use core::mem;
use core::ptr::{self, NonNull};
use std::env;
use std::ffi::CString;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use nadir_core::log::{info, warn};
use nadir_sys::dynlib::Library as _;
use nadir_sys::linux::alsa::{
    Asound, SND_PCM_ACCESS_RW_INTERLEAVED, SND_PCM_FORMAT_FLOAT_LE, SND_PCM_NONBLOCK,
    SND_PCM_STREAM_PLAYBACK, snd_lib_error_set_local_fn, snd_pcm_t, snd_pcm_uframes_t,
};
use nadir_sys::linux::libc::{EAGAIN, EPIPE};

use super::library::Library;
use crate::PlatformError;
use crate::audio::{AudioOutputConfig, AudioRender};

/// The file the loader looks for.
const LIBASOUND: &CStr = c"libasound.so.2";

/// The same, for messages.
const LIBASOUND_NAME: &str = "libasound.so.2";

/// The device a stream opens: the user's choice, which a sound server
/// usually provides.
const DEFAULT: &CStr = c"default";

/// Names the ALSA device to open in place of [`DEFAULT`], when it is set
/// and not empty.
const DEVICE_VARIABLE: &str = "NADIR_ALSA_DEVICE";

/// How long a lost device waits between tries to open it again.
const RETRY: Duration = Duration::from_secs(1);

/// The longest one wait for room lasts, so that a stop is seen.
const WAIT_MS: c_int = 20;

/// How long a device may take no frames before it is lost: longer than any
/// pause of a sound server that moves a stream to another device.
const STALL: Duration = Duration::from_secs(2);

/// How long opening a stream waits for its device to open.
const OPEN_WAIT: Duration = Duration::from_secs(2);

/// How long dropping a stream waits for its thread.
const JOIN_WAIT: Duration = Duration::from_secs(2);

/// How many errors in a row a write may meet before the device is given
/// up on. A real underrun or suspend is recovered in one; a device whose
/// every write fails, though it recovers, would otherwise hold the thread
/// in one period for ever, and stop the clock of what plays.
const MAX_FAILURES: u32 = 4;

/// libasound, loaded once for the whole process.
struct Alsa {
    asound: Asound,
    /// `snd_lib_error_set_local`, which older libraries lack.
    set_local: Option<snd_lib_error_set_local_fn>,
}

static ALSA: OnceLock<Result<Alsa, PlatformError>> = OnceLock::new();

/// Loads libasound, the first time it is called.
fn alsa() -> Result<&'static Alsa, PlatformError> {
    let loaded = ALSA.get_or_init(|| {
        // SAFETY: libasound is a system library that is sound to load into
        // any process.
        let library = unsafe { Library::open(LIBASOUND) }.map_err(|reason| {
            PlatformError::LibraryNotFound {
                library: LIBASOUND_NAME,
                reason,
            }
        })?;
        // SAFETY: the library is libasound, which defines these functions
        // as its headers declare them, and a `Library` is never unloaded.
        let asound =
            unsafe { Asound::load(&library) }.map_err(|missing| PlatformError::MissingSymbols {
                library: LIBASOUND_NAME,
                symbols: missing.names,
            })?;
        let found = library.symbol(c"snd_lib_error_set_local");
        let set_local = (!found.is_null()).then(|| {
            // SAFETY: the pointer is not null, and is libasound's
            // `snd_lib_error_set_local`, of this type. It stays valid, as
            // the library is never unloaded.
            unsafe { mem::transmute::<*const c_void, snd_lib_error_set_local_fn>(found) }
        });
        Ok(Alsa { asound, set_local })
    });
    loaded.as_ref().map_err(Clone::clone)
}

/// The ALSA device to open: `NADIR_ALSA_DEVICE`, or [`DEFAULT`].
fn device_name() -> CString {
    use std::os::unix::ffi::OsStringExt as _;

    match env::var_os(DEVICE_VARIABLE) {
        Some(name) if !name.is_empty() => {
            CString::new(name.into_vec()).expect("an environment variable has no NUL")
        }
        _ => DEFAULT.to_owned(),
    }
}

/// How long the stream waits, and for what. The tests shorten them.
#[derive(Clone, Copy, Debug)]
struct Timing {
    retry: Duration,
    wait_ms: c_int,
    stall: Duration,
    open_wait: Duration,
    join_wait: Duration,
}

const TIMING: Timing = Timing {
    retry: RETRY,
    wait_ms: WAIT_MS,
    stall: STALL,
    open_wait: OPEN_WAIT,
    join_wait: JOIN_WAIT,
};

/// The time, as the loop reads it and waits for it, so that the tests can
/// run it on a clock of their own.
trait Clock {
    fn now(&self) -> Instant;
    fn sleep_until(&self, deadline: Instant);
}

/// The system's clock.
struct Wall;

impl Clock for Wall {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn sleep_until(&self, deadline: Instant) {
        thread::sleep(deadline.saturating_duration_since(Instant::now()));
    }
}

/// What the device took, or what a stream with no device renders.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Format {
    sample_rate: u32,
    channels: u16,
    period_frames: usize,
    buffer_frames: usize,
}

impl Format {
    /// What `snd_pcm_set_params` asks of a device for `config`: a buffer of
    /// its latency, in periods of a quarter of it.
    fn nominal(config: &AudioOutputConfig) -> Self {
        let buffer = u64::from(config.sample_rate) * u64::from(config.latency_ms) / 1000;
        let buffer_frames = usize::try_from(buffer).unwrap_or(usize::MAX);
        Self {
            sample_rate: config.sample_rate,
            channels: config.channels,
            period_frames: (buffer_frames / 4).max(1),
            buffer_frames,
        }
    }

    /// The samples of a period.
    fn samples(self) -> usize {
        self.period_frames * usize::from(self.channels)
    }

    /// How long `frames` frames play: at most a day, which nothing here
    /// comes near.
    fn time_of(self, frames: usize) -> Duration {
        let seconds = frames as f64 / f64::from(self.sample_rate.max(1));
        Duration::from_secs_f64(seconds.min(86_400.0))
    }

    /// How long a period plays.
    fn period(self) -> Duration {
        self.time_of(self.period_frames)
    }
}

/// How the first open went, as the thread tells [`start`].
enum Opening {
    /// Not done yet.
    Waiting,
    /// The device opened, or did not and is tried again: the stream plays.
    Started,
    /// The device opened, but refused the format.
    Refused(PlatformError),
    /// `start` stopped waiting: the thread goes on as if there were no
    /// device, and a refusal is logged rather than returned.
    Abandoned,
}

/// The first open's outcome, behind a lock that `start` waits on.
struct Handshake {
    opening: Mutex<Opening>,
    changed: Condvar,
}

impl Handshake {
    fn lock(&self) -> MutexGuard<'_, Opening> {
        self.opening.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The audio thread's end, however it comes: a return or a panic. It is the
/// first local of the thread, so it drops last, after the render.
struct Ending {
    buffer_frames: Arc<AtomicUsize>,
    handshake: Arc<Handshake>,
    /// Disconnects the stream's `finished` when this is dropped.
    _done: mpsc::Sender<()>,
}

impl Drop for Ending {
    fn drop(&mut self) {
        // No device plays from here.
        self.buffer_frames.store(0, Ordering::Relaxed);
        let mut opening = self.handshake.lock();
        if matches!(*opening, Opening::Waiting) {
            *opening = Opening::Refused(PlatformError::AudioCallFailed {
                call: "the audio thread",
                error: "it ended before it had opened the device".to_string(),
            });
            self.handshake.changed.notify_all();
        }
    }
}

/// The audio thread of a stream. Dropping it stops the thread, and the
/// thread closes the device.
pub(crate) struct Stream {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    /// Disconnects when the thread ends, however it ends. In a mutex only
    /// so that the stream is `Sync`: the drop alone reads it.
    finished: Mutex<mpsc::Receiver<()>>,
    /// The frames of the device's buffer, or 0 while there is no device.
    buffer_frames: Arc<AtomicUsize>,
    sample_rate: u32,
    join_wait: Duration,
}

impl Stream {
    pub(crate) fn latency(&self) -> Option<Duration> {
        let frames = self.buffer_frames.load(Ordering::Relaxed);
        (frames > 0)
            .then(|| Duration::from_secs_f64(frames as f64 / f64::from(self.sample_rate.max(1))))
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        // Only the flag is shared, so no ordering is needed beyond its own.
        self.stop.store(true, Ordering::Relaxed);
        let Some(thread) = self.thread.take() else {
            return;
        };
        let finished = self
            .finished
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner);
        match finished.recv_timeout(self.join_wait) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => {
                // A thread that panicked, in a render that panicked, has
                // had its panic reported by the hook; passing it on from a
                // drop could abort the process.
                let _ = thread.join();
            }
            // ALSA holds it, as `snd_pcm_recover` does until the system
            // resumes, or a plugin's open does until its server answers.
            // Dropping the handle lets it go.
            Err(RecvTimeoutError::Timeout) => warn!(
                "the audio thread did not stop within {} s: it is left to end with the process",
                self.join_wait.as_secs_f64()
            ),
        }
    }
}

/// Opens the audio output on a new audio thread: see
/// [`open_audio`](crate::open_audio). The config is valid.
pub(crate) fn open(
    config: &AudioOutputConfig,
    render: Box<dyn AudioRender>,
) -> Result<Stream, PlatformError> {
    start(device_name(), *config, render, TIMING)
}

/// Opens the device `name` on a new audio thread.
fn start(
    name: CString,
    config: AudioOutputConfig,
    render: Box<dyn AudioRender>,
    timing: Timing,
) -> Result<Stream, PlatformError> {
    let alsa = alsa()?;
    let stop = Arc::new(AtomicBool::new(false));
    let buffer_frames = Arc::new(AtomicUsize::new(0));
    let handshake = Arc::new(Handshake {
        opening: Mutex::new(Opening::Waiting),
        changed: Condvar::new(),
    });
    let (done, finished) = mpsc::channel::<()>();
    let ending = Ending {
        buffer_frames: Arc::clone(&buffer_frames),
        handshake: Arc::clone(&handshake),
        _done: done,
    };
    let thread = {
        let stop = Arc::clone(&stop);
        let buffer_frames = Arc::clone(&buffer_frames);
        let handshake = Arc::clone(&handshake);
        thread::Builder::new()
            .name("nadir-audio".to_string())
            .spawn(move || {
                let _ending = ending;
                // Declared after `_ending`, so dropped before it, on a
                // return or a panic: the stream's drop waits for the
                // render's drop too.
                let mut render = render;
                run(
                    alsa,
                    &name,
                    &config,
                    render.as_mut(),
                    &stop,
                    &buffer_frames,
                    &handshake,
                    timing,
                );
            })
            .map_err(|error| PlatformError::AudioCallFailed {
                call: "std::thread::Builder::spawn",
                error: error.to_string(),
            })?
    };
    // From here, dropping the stream stops and joins the thread.
    let stream = Stream {
        stop,
        thread: Some(thread),
        finished: Mutex::new(finished),
        buffer_frames,
        sample_rate: config.sample_rate,
        join_wait: timing.join_wait,
    };
    let opening = handshake.lock();
    let (mut opening, _) = handshake
        .changed
        .wait_timeout_while(opening, timing.open_wait, |opening| {
            matches!(opening, Opening::Waiting)
        })
        .unwrap_or_else(PoisonError::into_inner);
    match mem::replace(&mut *opening, Opening::Abandoned) {
        Opening::Refused(error) => Err(error),
        Opening::Waiting => {
            warn!(
                "the audio device is slow to open: going on without it for now, after {} s",
                timing.open_wait.as_secs_f64()
            );
            Ok(stream)
        }
        Opening::Started | Opening::Abandoned => Ok(stream),
    }
}

/// The audio thread: the first open, whose outcome goes to `start`, then
/// [`play`].
#[expect(
    clippy::too_many_arguments,
    reason = "each is one of the thread's inputs"
)]
fn run(
    alsa: &'static Alsa,
    name: &CStr,
    config: &AudioOutputConfig,
    render: &mut dyn AudioRender,
    stop: &AtomicBool,
    buffer_frames: &AtomicUsize,
    handshake: &Handshake,
    timing: Timing,
) {
    let retry = timing.retry.as_secs_f64();
    let (device, refused) = match Pcm::open(alsa, name, config) {
        Ok(open) => {
            // Before `start` returns, so that the stream it gives has its
            // latency.
            buffer_frames.store(open.1.buffer_frames, Ordering::Relaxed);
            (Some(open), None)
        }
        // No device, or one that another program holds: as a device lost.
        Err(failure) if failure.call == "snd_pcm_open" => {
            warn!(
                "no audio device: {}. Trying to open it again every {retry} s",
                failure.describe(&alsa.asound, name),
            );
            (None, None)
        }
        Err(failure) => (None, Some(failure)),
    };
    {
        let mut opening = handshake.lock();
        match (&*opening, refused) {
            (Opening::Waiting, Some(failure)) => {
                *opening = Opening::Refused(failure.into_error(&alsa.asound, name));
                handshake.changed.notify_all();
                return;
            }
            (Opening::Waiting, None) => {
                *opening = Opening::Started;
                handshake.changed.notify_all();
            }
            (_, Some(failure)) => warn!(
                "the audio device opened, but did not take the format: {}. Trying again every \
                 {retry} s",
                failure.describe(&alsa.asound, name),
            ),
            (_, None) => {}
        }
    }
    let mut refusal = None;
    let reopen = || match quietly(alsa, || Pcm::open(alsa, name, config)) {
        Ok(open) => {
            refusal = None;
            Some(open)
        }
        Err(failure) => {
            if is_new_refusal(&mut refusal, failure) {
                warn!(
                    "the audio device opened, but did not take the format: {}. Trying again \
                     every {retry} s",
                    failure.describe(&alsa.asound, name),
                );
            }
            None
        }
    };
    play(
        device,
        Format::nominal(config),
        reopen,
        |error| describe(&alsa.asound, error),
        render,
        &Shared {
            stop,
            buffer_frames,
        },
        timing,
        &Wall,
    );
}

/// Whether a try's `failure` is a device that opened and refused the
/// format, other than the try before: such a refusal is logged once, until
/// it changes, or the device goes, or opens.
fn is_new_refusal(last: &mut Option<OpenFailure>, failure: OpenFailure) -> bool {
    let refused = (failure.call != "snd_pcm_open").then_some(failure);
    let new = refused.is_some() && refused != *last;
    *last = refused;
    new
}

/// Runs `call` with alsa-lib's messages to stderr silenced on this thread,
/// where the library can: a device tried again every second would print
/// the same ones each time, and a recovery that fails prints whatever its
/// `silent`. The first open, at the start, prints them.
fn quietly<T>(alsa: &Alsa, call: impl FnOnce() -> T) -> T {
    /// Drops every message.
    extern "C" fn quiet(
        _file: *const c_char,
        _line: c_int,
        _func: *const c_char,
        _err: c_int,
        _fmt: *const c_char,
        _arg: *mut c_void,
    ) {
    }

    let Some(set_local) = alsa.set_local else {
        return call();
    };
    // SAFETY: `set_local` is libasound's, and `quiet` is a function of the
    // handler's type that lives as long as the program.
    let previous = unsafe { set_local(Some(quiet)) };
    let result = call();
    // SAFETY: as above; `previous` is the handler that was set, or none.
    unsafe { set_local(previous) };
    result
}

/// What the loop shares with the stream.
struct Shared<'a> {
    stop: &'a AtomicBool,
    buffer_frames: &'a AtomicUsize,
}

/// A device that the audio thread writes to, as [`play`] sees it, so that
/// the tests can stand one in.
trait Device {
    /// Writes up to `samples.len() / channels` interleaved frames, without
    /// waiting, and returns how many it wrote, which can be 0, or a
    /// negative `errno` value: `-EAGAIN` when there is too little room.
    fn write(&mut self, samples: &[f32], channels: usize) -> Result<usize, c_int>;

    /// Waits up to `ms` milliseconds for room, or returns a negative
    /// `errno` value.
    fn wait(&mut self, ms: c_int) -> Result<(), c_int>;

    /// Recovers from `error`, which [`Device::write`] or [`Device::wait`]
    /// returned, or returns the error that it could not recover from.
    fn recover(&mut self, error: c_int) -> Result<(), c_int>;
}

/// A device given up on: the call that failed, and its error, or `None`
/// for a device that took no frames for the stall's time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Lost {
    call: &'static str,
    error: Option<c_int>,
}

/// Frames written since a device last made the loop wait for room. A
/// device that never does, as ALSA's `null`, takes frames as fast as they
/// come: the loop sleeps once they are more than a buffer ahead of the
/// clock, so that what plays keeps time with the wall.
struct Unpaced {
    since: Instant,
    frames: usize,
}

/// The audio thread's loop: render a period, write it, and again, until
/// the stop. A device that is lost, or `None` at the start, is waited for
/// with [`wait_for_device`], which keeps time until `reopen` gives one.
#[expect(
    clippy::too_many_arguments,
    reason = "each is one of the loop's inputs"
)]
fn play<D: Device>(
    mut device: Option<(D, Format)>,
    nominal: Format,
    mut reopen: impl FnMut() -> Option<(D, Format)>,
    describe: impl Fn(c_int) -> String,
    render: &mut dyn AudioRender,
    shared: &Shared<'_>,
    timing: Timing,
    clock: &dyn Clock,
) {
    let mut format = device.as_ref().map_or(nominal, |&(_, format)| format);
    let mut buffer = vec![0.0; format.samples()];
    loop {
        if let Some((mut open, _)) = device.take() {
            shared
                .buffer_frames
                .store(format.buffer_frames, Ordering::Relaxed);
            let channels = usize::from(format.channels);
            let mut unpaced = Unpaced {
                since: clock.now(),
                frames: 0,
            };
            let lost = loop {
                if shared.stop.load(Ordering::Relaxed) {
                    return;
                }
                render.render(&mut buffer, format.channels, format.sample_rate);
                let written = write_period(
                    &mut open,
                    &buffer,
                    channels,
                    render,
                    shared.stop,
                    timing,
                    clock,
                );
                match written {
                    Err(lost) => break lost,
                    // The device keeps the time.
                    Ok(true) => {
                        unpaced = Unpaced {
                            since: clock.now(),
                            frames: 0,
                        };
                    }
                    Ok(false) => {
                        unpaced.frames += format.period_frames;
                        let due = unpaced.since + format.time_of(unpaced.frames);
                        let ahead = due.checked_sub(format.time_of(format.buffer_frames));
                        if let Some(until) = ahead.filter(|&until| until > clock.now()) {
                            clock.sleep_until(until);
                        }
                    }
                }
            };
            shared.buffer_frames.store(0, Ordering::Relaxed);
            drop(open);
            let reason = lost.error.map_or_else(
                || format!("it took no frames for {} s", timing.stall.as_secs_f64()),
                &describe,
            );
            warn!(
                "the audio device stopped: {} failed: {reason}. Trying to open it again every {} \
                 s",
                lost.call,
                timing.retry.as_secs_f64(),
            );
        }
        let since = clock.now();
        let Some((found, found_format)) = wait_for_device(
            &mut buffer,
            format,
            &mut reopen,
            render,
            shared.stop,
            timing,
            clock,
        ) else {
            return;
        };
        info!(
            "the audio device is open, after {:.1} s without one",
            clock.now().duration_since(since).as_secs_f64()
        );
        format = found_format;
        // The period can change when the device opens again.
        buffer.resize(format.samples(), 0.0);
        device = Some((found, found_format));
    }
}

/// Writes all of `buffer`, waiting for room and recovering from underruns
/// and suspends, until it is written or the stop. Returns whether the
/// device made it wait for room.
fn write_period<D: Device>(
    device: &mut D,
    buffer: &[f32],
    channels: usize,
    render: &mut dyn AudioRender,
    stop: &AtomicBool,
    timing: Timing,
    clock: &dyn Clock,
) -> Result<bool, Lost> {
    let mut rest = buffer;
    let mut failures = 0;
    let mut waited = false;
    let mut progress = clock.now();
    while !rest.is_empty() && !stop.load(Ordering::Relaxed) {
        let (call, error) = match device.write(rest, channels) {
            Ok(frames) if frames > 0 => {
                failures = 0;
                progress = clock.now();
                rest = &rest[frames.saturating_mul(channels).min(rest.len())..];
                continue;
            }
            Err(error) if error != -EAGAIN => ("snd_pcm_writei", error),
            // Too little room, or nothing taken: wait for room.
            _ => {
                if clock.now().duration_since(progress) >= timing.stall {
                    return Err(Lost {
                        call: "snd_pcm_writei",
                        error: None,
                    });
                }
                waited = true;
                match device.wait(timing.wait_ms) {
                    Ok(()) => continue,
                    Err(error) => ("snd_pcm_wait", error),
                }
            }
        };
        if error == -EPIPE {
            render.underrun();
        }
        failures += 1;
        if failures == MAX_FAILURES {
            return Err(Lost {
                call,
                error: Some(error),
            });
        }
        if let Err(failed) = device.recover(error) {
            // `snd_pcm_recover` returns the errors it does not handle
            // unchanged: then it is the write or the wait that failed.
            let call = if failed == error {
                call
            } else {
                "snd_pcm_recover"
            };
            return Err(Lost {
                call,
                error: Some(failed),
            });
        }
    }
    Ok(waited)
}

/// While there is no device: renders a period at a time, in time with the
/// period, so that what plays keeps time, and tries `reopen` every retry.
/// Returns the device once it opens, or `None` at the stop.
fn wait_for_device<D>(
    buffer: &mut [f32],
    format: Format,
    reopen: &mut impl FnMut() -> Option<(D, Format)>,
    render: &mut dyn AudioRender,
    stop: &AtomicBool,
    timing: Timing,
    clock: &dyn Clock,
) -> Option<(D, Format)> {
    let period = format.period();
    let mut deadline = clock.now();
    let mut next_try = deadline + timing.retry;
    while !stop.load(Ordering::Relaxed) {
        render.render(buffer, format.channels, format.sample_rate);
        deadline += period;
        let now = clock.now();
        if deadline > now {
            clock.sleep_until(deadline);
        } else {
            // Behind, after a slow try: go on from now, rather than render
            // the missed periods at once.
            deadline = now;
        }
        if clock.now() >= next_try {
            if let Some(found) = reopen() {
                return Some(found);
            }
            next_try = clock.now() + timing.retry;
        }
    }
    None
}

/// Why a device did not open: the call that failed, and its error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OpenFailure {
    call: &'static str,
    failure: Failure,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Failure {
    /// A negative error code that the call returned.
    Code(c_int),
    /// What else was wrong with what it returned.
    Message(&'static str),
}

impl OpenFailure {
    fn code(call: &'static str, error: c_int) -> Self {
        Self {
            call,
            failure: Failure::Code(error),
        }
    }

    fn message(call: &'static str, message: &'static str) -> Self {
        Self {
            call,
            failure: Failure::Message(message),
        }
    }

    /// "`call` failed: what alsa-lib says (the ALSA device `name`)".
    fn describe(self, asound: &Asound, name: &CStr) -> String {
        self.into_error(asound, name).to_string()
    }

    fn into_error(self, asound: &Asound, name: &CStr) -> PlatformError {
        let reason = match self.failure {
            Failure::Code(error) => describe(asound, error),
            Failure::Message(message) => message.to_string(),
        };
        PlatformError::AudioCallFailed {
            call: self.call,
            error: format!("{reason} (the ALSA device `{}`)", name.to_string_lossy()),
        }
    }
}

/// An open PCM, closed when this is dropped.
///
/// It is opened, used and closed on the audio thread only: it holds a raw
/// pointer, so it is neither `Send` nor `Sync`.
struct Pcm {
    alsa: &'static Alsa,
    pcm: NonNull<snd_pcm_t>,
}

impl Pcm {
    /// Opens the device `name` for playback, in the mode in which neither
    /// the kernel's device nor a write waits, and sets it to interleaved
    /// `f32` frames of `config`'s rate and channels.
    fn open(
        alsa: &'static Alsa,
        name: &CStr,
        config: &AudioOutputConfig,
    ) -> Result<(Self, Format), OpenFailure> {
        let asound = &alsa.asound;
        let mut pcm = ptr::null_mut();
        // SAFETY: `pcm` is a place for the handle, and `name` ends with a
        // NUL.
        let result = unsafe {
            (asound.snd_pcm_open)(
                &raw mut pcm,
                name.as_ptr(),
                SND_PCM_STREAM_PLAYBACK,
                SND_PCM_NONBLOCK,
            )
        };
        if result < 0 {
            return Err(OpenFailure::code("snd_pcm_open", result));
        }
        let Some(pcm) = NonNull::new(pcm) else {
            return Err(OpenFailure::message("snd_pcm_open", "it gave no device"));
        };
        // From here, dropping `pcm` closes it.
        let pcm = Self { alsa, pcm };

        // SAFETY: the PCM is open, on this thread. `soft_resample` 1 lets
        // alsa-lib resample to a rate the device lacks. The latency is at
        // most 500 ms, so its microseconds fit.
        let result = unsafe {
            (asound.snd_pcm_set_params)(
                pcm.pcm.as_ptr(),
                SND_PCM_FORMAT_FLOAT_LE,
                SND_PCM_ACCESS_RW_INTERLEAVED,
                c_uint::from(config.channels),
                config.sample_rate,
                1,
                config.latency_ms.saturating_mul(1000),
            )
        };
        if result < 0 {
            return Err(OpenFailure::code("snd_pcm_set_params", result));
        }

        let mut buffer_frames: snd_pcm_uframes_t = 0;
        let mut period_frames: snd_pcm_uframes_t = 0;
        // SAFETY: the PCM is open and set, on this thread, and both
        // pointers are places for a count.
        let result = unsafe {
            (asound.snd_pcm_get_params)(
                pcm.pcm.as_ptr(),
                &raw mut buffer_frames,
                &raw mut period_frames,
            )
        };
        if result < 0 {
            return Err(OpenFailure::code("snd_pcm_get_params", result));
        }
        let (Ok(period_frames), Ok(buffer_frames)) = (
            usize::try_from(period_frames),
            usize::try_from(buffer_frames),
        ) else {
            return Err(OpenFailure::message(
                "snd_pcm_get_params",
                "it gave more frames than memory holds",
            ));
        };
        if period_frames == 0 || period_frames > buffer_frames {
            return Err(OpenFailure::message(
                "snd_pcm_get_params",
                "it gave a period of no frames, or longer than the buffer",
            ));
        }
        // `snd_pcm_set_params` fails unless the device takes exactly this
        // rate and these channels.
        let format = Format {
            sample_rate: config.sample_rate,
            channels: config.channels,
            period_frames,
            buffer_frames,
        };
        Ok((pcm, format))
    }
}

impl Device for Pcm {
    fn write(&mut self, samples: &[f32], channels: usize) -> Result<usize, c_int> {
        let frames = samples.len() / channels;
        // SAFETY: the PCM is open, on this thread, and set to interleaved
        // frames of `channels` little-endian `f32`s, which is the layout of
        // `f32` here (`nadir_sys::linux::alsa` asserts it); `samples` holds
        // `frames` of them.
        let written = unsafe {
            (self.alsa.asound.snd_pcm_writei)(
                self.pcm.as_ptr(),
                samples.as_ptr().cast(),
                frames as snd_pcm_uframes_t,
            )
        };
        if written < 0 {
            // ALSA's error codes are `errno` values, which fit an `int`.
            Err(c_int::try_from(written).unwrap_or(c_int::MIN))
        } else {
            Ok(usize::try_from(written).unwrap_or(frames))
        }
    }

    fn wait(&mut self, ms: c_int) -> Result<(), c_int> {
        // SAFETY: the PCM is open, on this thread.
        let result = unsafe { (self.alsa.asound.snd_pcm_wait)(self.pcm.as_ptr(), ms) };
        if result < 0 { Err(result) } else { Ok(()) }
    }

    fn recover(&mut self, error: c_int) -> Result<(), c_int> {
        let recover = self.alsa.asound.snd_pcm_recover;
        let pcm = self.pcm.as_ptr();
        // SAFETY: the PCM is open, on this thread, and `error` is what a
        // call on it returned. `silent` 1 keeps back only the notice of an
        // underrun; a failure to prepare the device again is printed, which
        // `quietly` keeps back too.
        let result = quietly(self.alsa, || unsafe { recover(pcm, error, 1) });
        if result < 0 { Err(result) } else { Ok(()) }
    }
}

impl Drop for Pcm {
    fn drop(&mut self) {
        // SAFETY: the PCM is open, on this thread, and is not used again.
        let result = unsafe { (self.alsa.asound.snd_pcm_close)(self.pcm.as_ptr()) };
        // The handle is freed whatever the result, and nothing is left to
        // do about a failure to close it.
        let _ = result;
    }
}

/// What alsa-lib says of an error code.
fn describe(asound: &Asound, error: c_int) -> String {
    // SAFETY: `snd_strerror` takes any number.
    let message = unsafe { (asound.snd_strerror)(error) };
    if message.is_null() {
        return format!("error {error}");
    }
    // SAFETY: the string is not null and ends with a NUL. It is alsa-lib's
    // or the C library's, which outlives this call, and it is copied
    // before anything else could change it.
    let message = unsafe { CStr::from_ptr(message) };
    message.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use nadir_core::log;
    use nadir_sys::linux::libc::ESTRPIPE;

    use super::*;

    /// Interrupted system call. `asm-generic/errno-base.h`: `#define EINTR 4`
    const EINTR: c_int = 4;
    /// I/O error. `asm-generic/errno-base.h`: `#define EIO 5`
    const EIO: c_int = 5;
    /// No such device, as a write to an unplugged device returns.
    /// `asm-generic/errno-base.h`: `#define ENODEV 19`
    const ENODEV: c_int = 19;

    /// A small period, of 4 frames.
    const FORMAT: Format = Format {
        sample_rate: 48_000,
        channels: 2,
        period_frames: 4,
        buffer_frames: 16,
    };

    /// Retries and stalls that tests can count in milliseconds.
    const FAST: Timing = Timing {
        retry: Duration::from_millis(10),
        wait_ms: 20,
        stall: Duration::from_millis(100),
        open_wait: Duration::from_millis(100),
        join_wait: Duration::from_millis(100),
    };

    /// A clock that moves only when it is slept on, or moved.
    struct FakeClock {
        start: Instant,
        now: Mutex<Instant>,
    }

    impl FakeClock {
        fn new() -> Arc<Self> {
            let start = Instant::now();
            Arc::new(Self {
                start,
                now: Mutex::new(start),
            })
        }

        fn advance(&self, by: Duration) {
            *self.now.lock().expect("no test panics while holding it") += by;
        }

        /// The time since the clock was made.
        fn elapsed(&self) -> Duration {
            self.now() - self.start
        }
    }

    impl Clock for FakeClock {
        fn now(&self) -> Instant {
            *self.now.lock().expect("no test panics while holding it")
        }

        fn sleep_until(&self, deadline: Instant) {
            let mut now = self.now.lock().expect("no test panics while holding it");
            *now = (*now).max(deadline);
        }
    }

    /// What happened, in order.
    #[derive(Clone, Debug, PartialEq)]
    enum Seen {
        /// The `n`th render, of so many samples.
        Render(usize, usize),
        Underrun,
        /// A write that took so many frames.
        Wrote(usize),
        Wait,
        Recover(c_int),
    }

    type Log = Arc<Mutex<Vec<Seen>>>;

    fn seen(log: &Log) -> Vec<Seen> {
        log.lock().expect("no test panics while holding it").clone()
    }

    fn note(log: &Log, seen: Seen) {
        log.lock()
            .expect("no test panics while holding it")
            .push(seen);
    }

    /// How many renders `log` has.
    fn renders(log: &Log) -> usize {
        seen(log)
            .iter()
            .filter(|seen| matches!(seen, Seen::Render(..)))
            .count()
    }

    /// A render that writes the `n`th render's `i`th sample as
    /// `n * 1000 + i`, and notes each call.
    struct Counting {
        renders: usize,
        log: Log,
    }

    impl Counting {
        fn new(log: &Log) -> Self {
            Self {
                renders: 0,
                log: Arc::clone(log),
            }
        }
    }

    /// The samples of the `n`th render of `len` samples.
    fn rendered(n: usize, len: usize) -> Vec<f32> {
        (0..len).map(|i| (n * 1000 + i) as f32).collect()
    }

    impl AudioRender for Counting {
        fn render(&mut self, out: &mut [f32], channels: u16, sample_rate: u32) {
            assert_eq!((channels, sample_rate), (2, 48_000));
            for (i, sample) in out.iter_mut().enumerate() {
                *sample = (self.renders * 1000 + i) as f32;
            }
            note(&self.log, Seen::Render(self.renders, out.len()));
            self.renders += 1;
        }

        fn underrun(&mut self) {
            note(&self.log, Seen::Underrun);
        }
    }

    /// What the fake device does next.
    #[derive(Clone, Copy, Debug)]
    enum Step {
        /// A write takes up to so many frames.
        Take(usize),
        /// A write fails with this error, negated as ALSA returns it.
        Fail(c_int),
        /// A write finds no room, and the wait after it times out.
        Busy,
        /// A write finds no room, and the wait after it fails so.
        BusyThen(c_int),
    }

    /// A device that follows a script, then stops the loop.
    struct Fake {
        steps: VecDeque<Step>,
        /// The error of the wait after the last write, if it fails.
        wait_fails: Option<c_int>,
        /// What `recover` returns for an error it handles; `None` is 0.
        recover_fails: Option<c_int>,
        written: Arc<Mutex<Vec<f32>>>,
        log: Log,
        stop: Arc<AtomicBool>,
        clock: Arc<FakeClock>,
        closed: Arc<AtomicBool>,
    }

    impl Fake {
        fn new(steps: &[Step], test: &Test) -> Self {
            Self {
                steps: steps.iter().copied().collect(),
                wait_fails: None,
                recover_fails: None,
                written: Arc::default(),
                log: Arc::clone(&test.log),
                stop: Arc::clone(&test.stop),
                clock: Arc::clone(&test.clock),
                closed: Arc::default(),
            }
        }
    }

    impl Device for Fake {
        fn write(&mut self, samples: &[f32], channels: usize) -> Result<usize, c_int> {
            let step = self
                .steps
                .pop_front()
                .expect("the loop stops when the script ends");
            if self.steps.is_empty() {
                self.stop.store(true, Ordering::Relaxed);
            }
            match step {
                Step::Take(frames) => {
                    let frames = frames.min(samples.len() / channels);
                    self.written
                        .lock()
                        .expect("no test panics while holding it")
                        .extend_from_slice(&samples[..frames * channels]);
                    note(&self.log, Seen::Wrote(frames));
                    Ok(frames)
                }
                Step::Fail(error) => Err(error),
                Step::Busy => Err(-EAGAIN),
                Step::BusyThen(error) => {
                    self.wait_fails = Some(error);
                    Err(-EAGAIN)
                }
            }
        }

        fn wait(&mut self, ms: c_int) -> Result<(), c_int> {
            note(&self.log, Seen::Wait);
            if let Some(error) = self.wait_fails.take() {
                return Err(error);
            }
            let ms = u64::try_from(ms).expect("a wait is not negative");
            self.clock.advance(Duration::from_millis(ms));
            Ok(())
        }

        fn recover(&mut self, error: c_int) -> Result<(), c_int> {
            note(&self.log, Seen::Recover(error));
            // As `snd_pcm_recover`: these three, and no others.
            if ![-EINTR, -EPIPE, -ESTRPIPE].contains(&error) {
                return Err(error);
            }
            self.recover_fails.map_or(Ok(()), Err)
        }
    }

    impl Drop for Fake {
        fn drop(&mut self) {
            self.closed.store(true, Ordering::Relaxed);
        }
    }

    /// What one test's loop shares.
    struct Test {
        name: &'static str,
        log: Log,
        stop: Arc<AtomicBool>,
        buffer_frames: Arc<AtomicUsize>,
        clock: Arc<FakeClock>,
    }

    impl Test {
        fn new(name: &'static str) -> Self {
            Self {
                name,
                log: Log::default(),
                stop: Arc::default(),
                buffer_frames: Arc::default(),
                clock: FakeClock::new(),
            }
        }

        /// Runs the loop on this thread, from `device`, until it stops.
        fn run(
            &self,
            device: Option<(Fake, Format)>,
            nominal: Format,
            reopen: impl FnMut() -> Option<(Fake, Format)>,
            timing: Timing,
        ) {
            let name = self.name;
            play(
                device,
                nominal,
                reopen,
                |error| format!("{name}: fake error {error}"),
                &mut Counting::new(&self.log),
                &Shared {
                    stop: &self.stop,
                    buffer_frames: &self.buffer_frames,
                },
                timing,
                &*self.clock,
            );
        }

        /// Runs `device` with a `reopen` that stops the loop.
        fn run_until_lost(&self, device: Fake) {
            let stop = Arc::clone(&self.stop);
            let reopen = || {
                stop.store(true, Ordering::Relaxed);
                None
            };
            self.run(Some((device, FORMAT)), FORMAT, reopen, FAST);
        }

        /// Whether the log's last records include one of this test's fake
        /// errors that contains `text`: its name as the errors write it, so
        /// that `separated` does not find the lines of `unseparated`, which
        /// share the log.
        fn logged(&self, text: &str) -> bool {
            let error = format!(" {}: fake error ", self.name);
            log::tail(log::RING_CAPACITY)
                .iter()
                .any(|line| line.contains(&error) && line.contains(text))
        }
    }

    fn samples_of(written: &Arc<Mutex<Vec<f32>>>) -> Vec<f32> {
        written
            .lock()
            .expect("no test panics while holding it")
            .clone()
    }

    #[test]
    fn each_period_is_rendered_then_written_whole() {
        let test = Test::new("whole");
        let device = Fake::new(&[Step::Take(4), Step::Take(4), Step::Take(4)], &test);
        let (out, closed) = (Arc::clone(&device.written), Arc::clone(&device.closed));
        test.run_until_lost(device);

        assert_eq!(
            seen(&test.log),
            [
                Seen::Render(0, 8),
                Seen::Wrote(4),
                Seen::Render(1, 8),
                Seen::Wrote(4),
                Seen::Render(2, 8),
                Seen::Wrote(4),
            ]
        );
        assert_eq!(
            samples_of(&out),
            [rendered(0, 8), rendered(1, 8), rendered(2, 8)].concat()
        );
        assert!(closed.load(Ordering::Relaxed), "the device is closed");
        // While it played, the stream had its buffer.
        assert_eq!(test.buffer_frames.load(Ordering::Relaxed), 16);
    }

    #[test]
    fn a_full_buffer_is_waited_for_and_the_rest_written() {
        let test = Test::new("full");
        let steps = [
            Step::Take(1),
            Step::Busy,
            Step::Busy,
            Step::Take(9),
            Step::Take(4),
        ];
        let device = Fake::new(&steps, &test);
        let out = Arc::clone(&device.written);
        test.run_until_lost(device);

        assert_eq!(
            seen(&test.log),
            [
                Seen::Render(0, 8),
                Seen::Wrote(1),
                Seen::Wait,
                Seen::Wait,
                Seen::Wrote(3),
                Seen::Render(1, 8),
                Seen::Wrote(4),
            ]
        );
        assert_eq!(samples_of(&out), [rendered(0, 8), rendered(1, 8)].concat());
    }

    #[test]
    fn an_underrun_is_counted_and_the_period_still_written_whole() {
        let test = Test::new("underrun");
        let steps = [
            Step::Take(4),
            Step::Fail(-EPIPE),
            Step::Take(4),
            Step::Take(1),
            Step::BusyThen(-EPIPE),
            Step::Take(3),
        ];
        let device = Fake::new(&steps, &test);
        let out = Arc::clone(&device.written);
        test.run_until_lost(device);

        assert_eq!(
            seen(&test.log),
            [
                Seen::Render(0, 8),
                Seen::Wrote(4),
                Seen::Render(1, 8),
                // Before the next render, as the trait promises.
                Seen::Underrun,
                Seen::Recover(-EPIPE),
                Seen::Wrote(4),
                Seen::Render(2, 8),
                Seen::Wrote(1),
                // The wait found it.
                Seen::Wait,
                Seen::Underrun,
                Seen::Recover(-EPIPE),
                Seen::Wrote(3),
            ]
        );
        assert_eq!(
            samples_of(&out),
            [rendered(0, 8), rendered(1, 8), rendered(2, 8)].concat()
        );
    }

    #[test]
    fn a_suspend_or_a_signal_is_recovered_with_no_underrun() {
        let test = Test::new("suspend");
        let steps = [
            Step::Fail(-ESTRPIPE),
            Step::Take(4),
            Step::Fail(-EINTR),
            Step::Take(4),
        ];
        let device = Fake::new(&steps, &test);
        let out = Arc::clone(&device.written);
        test.run_until_lost(device);

        assert_eq!(
            seen(&test.log),
            [
                Seen::Render(0, 8),
                Seen::Recover(-ESTRPIPE),
                Seen::Wrote(4),
                Seen::Render(1, 8),
                Seen::Recover(-EINTR),
                Seen::Wrote(4),
            ]
        );
        assert_eq!(samples_of(&out), [rendered(0, 8), rendered(1, 8)].concat());
    }

    #[test]
    fn a_device_that_takes_nothing_is_lost_once_the_stall_has_passed() {
        let test = Test::new("stalled");
        let mut steps = vec![Step::Take(4), Step::Take(1)];
        steps.extend([Step::Busy; 10]);
        let device = Fake::new(&steps, &test);
        let closed = Arc::clone(&device.closed);
        let stop = Arc::clone(&test.stop);
        let reopen = || {
            stop.store(true, Ordering::Relaxed);
            None
        };
        // A stall of this test's own, which names it in the log.
        let timing = Timing {
            stall: Duration::from_millis(120),
            ..FAST
        };
        test.run(Some((device, FORMAT)), FORMAT, reopen, timing);

        // The last frame taken, then waits of 20 ms until 120 ms had
        // passed without one: six of them.
        let log = seen(&test.log);
        let waits = log.iter().filter(|seen| **seen == Seen::Wait).count();
        assert_eq!(waits, 6, "{log:?}");
        assert!(closed.load(Ordering::Relaxed), "the device is closed");
        let lines = log::tail(log::RING_CAPACITY);
        let logged = lines.iter().any(|line| {
            line.contains(
                "the audio device stopped: snd_pcm_writei failed: it took no frames for 0.12 s",
            )
        });
        assert!(logged, "{lines:?}");
    }

    #[test]
    fn a_stop_is_seen_after_the_wait_in_which_it_comes() {
        let test = Test::new("stop-while-full");
        let device = Fake::new(&[Step::Busy; 3], &test);
        // The script's last write stops the loop, and the wait after it is
        // the last.
        test.run_until_lost(device);

        assert_eq!(
            seen(&test.log),
            [Seen::Render(0, 8), Seen::Wait, Seen::Wait, Seen::Wait]
        );
    }

    #[test]
    fn a_device_whose_errors_keep_coming_is_lost_at_the_fourth() {
        let test = Test::new("errors");
        // Each underrun recovers, and the next write fails again.
        let mut steps = vec![Step::Fail(-EPIPE); 4];
        steps.push(Step::Take(4));
        let device = Fake::new(&steps, &test);
        test.run_until_lost(device);

        assert!(
            test.logged("the audio device stopped: snd_pcm_writei failed: errors: fake error -32.")
        );
        let log = seen(&test.log);
        let recovered = [Seen::Underrun, Seen::Recover(-EPIPE)];
        assert_eq!(
            log[..8],
            [
                &[Seen::Render(0, 8)][..],
                &recovered,
                &recovered,
                &recovered,
                &[Seen::Underrun]
            ]
            .concat()
        );
        assert!(
            log[8..].iter().all(|seen| matches!(seen, Seen::Render(..))),
            "{log:?}"
        );
    }

    #[test]
    fn a_failure_of_the_recovery_itself_is_named() {
        let test = Test::new("recovery");
        let mut device = Fake::new(&[Step::Fail(-EPIPE), Step::Take(4)], &test);
        device.recover_fails = Some(-EIO);
        test.run_until_lost(device);

        assert!(
            test.logged(
                "the audio device stopped: snd_pcm_recover failed: recovery: fake error -5."
            )
        );
        let log = seen(&test.log);
        assert_eq!(
            log[..3],
            [Seen::Render(0, 8), Seen::Underrun, Seen::Recover(-EPIPE)]
        );
        assert!(
            log[3..].iter().all(|seen| matches!(seen, Seen::Render(..))),
            "{log:?}"
        );
    }

    #[test]
    fn a_stream_stopped_before_its_first_period_renders_nothing() {
        let test = Test::new("stopped");
        test.stop.store(true, Ordering::Relaxed);
        let device = Fake::new(&[Step::Take(4)], &test);
        let closed = Arc::clone(&device.closed);
        test.run_until_lost(device);

        assert_eq!(seen(&test.log), []);
        assert!(closed.load(Ordering::Relaxed), "the device is closed");
    }

    #[test]
    fn a_write_that_takes_nothing_is_waited_on_as_a_full_buffer() {
        let test = Test::new("nothing");
        let steps = [Step::Take(2), Step::Take(0), Step::Take(2), Step::Take(4)];
        let device = Fake::new(&steps, &test);
        test.run_until_lost(device);

        assert_eq!(
            seen(&test.log),
            [
                Seen::Render(0, 8),
                Seen::Wrote(2),
                Seen::Wrote(0),
                Seen::Wait,
                Seen::Wrote(2),
                Seen::Render(1, 8),
                Seen::Wrote(4),
            ]
        );
    }

    #[test]
    fn errors_with_frames_taken_between_them_do_not_add_up() {
        let test = Test::new("separated");
        let mut steps = [Step::Fail(-EPIPE), Step::Take(1)].repeat(4);
        steps.push(Step::Take(4));
        let device = Fake::new(&steps, &test);
        let out = Arc::clone(&device.written);
        test.run_until_lost(device);

        // Four underruns in one period, each recovered and followed by a
        // frame: the device plays on.
        let log = seen(&test.log);
        let underruns = log.iter().filter(|seen| **seen == Seen::Underrun).count();
        assert_eq!(underruns, 4, "{log:?}");
        assert_eq!(samples_of(&out), [rendered(0, 8), rendered(1, 8)].concat());
        assert!(!test.logged("the audio device stopped"));
    }

    #[test]
    fn errors_with_only_waits_between_them_add_up() {
        let test = Test::new("unseparated");
        let mut steps = [Step::Fail(-EPIPE), Step::Busy].repeat(3);
        steps.extend([Step::Fail(-EPIPE), Step::Take(4)]);
        let device = Fake::new(&steps, &test);
        test.run_until_lost(device);

        // Three waits of 20 ms: lost at the fourth error, not at the stall.
        assert!(test.logged(
            "the audio device stopped: snd_pcm_writei failed: unseparated: fake error -32."
        ));
    }

    #[test]
    fn a_slow_period_that_goes_on_taking_frames_is_no_stall() {
        let test = Test::new("slow-period");
        let mut steps = Vec::new();
        for _ in 0..3 {
            steps.push(Step::Take(1));
            steps.extend([Step::Busy; 3]);
        }
        steps.extend([Step::Take(1), Step::Take(4)]);
        let device = Fake::new(&steps, &test);
        let out = Arc::clone(&device.written);
        test.run_until_lost(device);

        // 180 ms on one period, past the stall of 100 ms, but a frame taken
        // every 60.
        assert_eq!(test.clock.elapsed(), Duration::from_millis(180));
        assert_eq!(samples_of(&out), [rendered(0, 8), rendered(1, 8)].concat());
        assert!(!test.logged("the audio device stopped"));
    }

    #[test]
    fn a_wait_that_fails_loses_the_device_and_is_named() {
        let test = Test::new("wait");
        let steps = [Step::Take(4), Step::BusyThen(-ENODEV), Step::Take(4)];
        let device = Fake::new(&steps, &test);
        test.run_until_lost(device);

        assert!(
            test.logged("the audio device stopped: snd_pcm_wait failed: wait: fake error -19.")
        );
    }

    #[test]
    fn the_first_device_s_own_period_and_buffer_are_played() {
        let test = Test::new("own-format");
        let device = Fake::new(&[Step::Take(8), Step::Take(8)], &test);
        let format = Format {
            period_frames: 8,
            buffer_frames: 32,
            ..FORMAT
        };
        let stop = Arc::clone(&test.stop);
        let reopen = || {
            stop.store(true, Ordering::Relaxed);
            None
        };
        test.run(Some((device, format)), FORMAT, reopen, FAST);

        assert_eq!(
            seen(&test.log),
            [
                Seen::Render(0, 16),
                Seen::Wrote(8),
                Seen::Render(1, 16),
                Seen::Wrote(8),
            ]
        );
        assert_eq!(test.buffer_frames.load(Ordering::Relaxed), 32);
    }

    #[test]
    fn a_device_that_never_waits_is_held_a_buffer_ahead_of_the_clock() {
        let test = Test::new("unpaced");
        let device = Fake::new(&[Step::Take(4); 40], &test);
        test.run_until_lost(device);

        // 40 periods of 4 frames, less the buffer of 16 that may be ahead:
        // 144 frames at 48 kHz.
        let elapsed = test.clock.elapsed();
        let expected = Duration::from_millis(3);
        assert!(
            elapsed.abs_diff(expected) < Duration::from_micros(1),
            "{elapsed:?}"
        );
        assert_eq!(renders(&test.log), 40);
    }

    #[test]
    fn a_device_that_makes_the_loop_wait_is_not_held_back_too() {
        let test = Test::new("paced");
        let device = Fake::new(&[Step::Busy, Step::Take(4)].repeat(40), &test);
        test.run_until_lost(device);

        // Only the waits moved the clock: 40 of 20 ms.
        assert_eq!(test.clock.elapsed(), Duration::from_millis(800));
    }

    /// 48 frames: a period of 1 ms.
    const MILLISECOND: Format = Format {
        sample_rate: 48_000,
        channels: 2,
        period_frames: 48,
        buffer_frames: 192,
    };

    #[test]
    fn a_lost_device_keeps_time_and_is_tried_every_retry_until_it_opens() {
        let test = Test::new("lost");
        let lost = Fake::new(
            &[Step::Take(48), Step::Fail(-ENODEV), Step::Take(48)],
            &test,
        );
        let closed = Arc::clone(&lost.closed);
        let mut tries = Vec::new();
        let reopen = || {
            assert!(closed.load(Ordering::Relaxed), "the lost device is closed");
            assert_eq!(test.buffer_frames.load(Ordering::Relaxed), 0, "no latency");
            tries.push((test.clock.elapsed(), renders(&test.log)));
            if tries.len() < 3 {
                return None;
            }
            let device = Fake::new(&[Step::Take(8), Step::Take(8)], &test);
            // A device that opens again may take another period.
            Some((
                device,
                Format {
                    period_frames: 8,
                    buffer_frames: 32,
                    ..FORMAT
                },
            ))
        };
        test.run(Some((lost, MILLISECOND)), MILLISECOND, reopen, FAST);

        // Lost at 0 ms, after the second render: then a render a
        // millisecond, and a try every 10.
        let ms = Duration::from_millis;
        assert_eq!(tries, [(ms(10), 12), (ms(20), 22), (ms(30), 32)]);
        assert!(test.logged(
            "the audio device stopped: snd_pcm_writei failed: lost: fake error -19. Trying to \
             open it again every 0.01 s"
        ));
        // The device that opened again plays its own periods.
        let log = seen(&test.log);
        assert_eq!(
            log[log.len() - 4..],
            [
                Seen::Render(32, 16),
                Seen::Wrote(8),
                Seen::Render(33, 16),
                Seen::Wrote(8),
            ]
        );
        assert_eq!(test.buffer_frames.load(Ordering::Relaxed), 32);
    }

    #[test]
    fn a_slow_try_loses_only_the_periods_it_took() {
        let test = Test::new("slow");
        // A step after the failure, so that the script does not end there.
        let lost = Fake::new(&[Step::Fail(-ENODEV), Step::Take(48)], &test);
        let mut tries = Vec::new();
        let reopen = || {
            tries.push((test.clock.elapsed(), renders(&test.log)));
            // Each try takes 5.5 ms.
            test.clock.advance(Duration::from_micros(5_500));
            if tries.len() == 2 {
                test.stop.store(true, Ordering::Relaxed);
            }
            None
        };
        test.run(Some((lost, MILLISECOND)), MILLISECOND, reopen, FAST);

        // The first render was the one the device lost. Then a render a
        // millisecond, and the first try at 10 ms, which ends at 15.5 ms.        // The render after it is behind, so it does not wait, and the clock
        // starts again from it: the next try is at 25.5 ms, 11 renders
        // later. Of the 15.5 periods since the first try, the 4.5 that the
        // slow try took are lost, and no more.
        let ms = |ms: f64| Duration::from_secs_f64(ms / 1000.0);
        assert_eq!(tries, [(ms(10.0), 11), (ms(25.5), 22)]);
    }

    #[test]
    fn a_stream_with_no_device_at_the_start_keeps_time_until_one_opens() {
        let test = Test::new("absent");
        let mut tries = 0;
        let reopen = || {
            tries += 1;
            (tries == 2).then(|| (Fake::new(&[Step::Take(48)], &test), MILLISECOND))
        };
        test.run(None, MILLISECOND, reopen, FAST);

        // Twenty renders of the nominal period while there was none, then
        // the device's.
        let log = seen(&test.log);
        assert_eq!(log.len(), 22, "{log:?}");
        assert!(
            log[..20]
                .iter()
                .enumerate()
                .all(|(n, seen)| *seen == Seen::Render(n, 96)),
            "{log:?}"
        );
        assert_eq!(log[20..], [Seen::Render(20, 96), Seen::Wrote(48)]);
        assert_eq!(tries, 2);
    }

    #[cfg(feature = "dev")]
    #[test]
    fn a_period_allocates_and_frees_nothing_even_with_underruns_and_waits() {
        use crate::{AllocCounts, thread_alloc_counts};

        /// Notes the counts at its first and last render, and nothing else.
        struct Quiet {
            first: Option<AllocCounts>,
            last: Option<AllocCounts>,
            underruns: usize,
        }

        impl AudioRender for Quiet {
            fn render(&mut self, _out: &mut [f32], _channels: u16, _sample_rate: u32) {
                let now = thread_alloc_counts();
                if self.first.is_none() {
                    self.first = Some(now);
                }
                self.last = Some(now);
            }

            fn underrun(&mut self) {
                self.underruns += 1;
            }
        }

        /// Takes whole periods, with a full buffer before every third and
        /// an underrun before every third after it.
        struct Steady {
            writes: usize,
            stop: Arc<AtomicBool>,
        }

        impl Device for Steady {
            fn write(&mut self, samples: &[f32], channels: usize) -> Result<usize, c_int> {
                self.writes += 1;
                if self.writes == 3_000 {
                    self.stop.store(true, Ordering::Relaxed);
                }
                match self.writes % 3 {
                    0 => Err(-EPIPE),
                    1 => Err(-EAGAIN),
                    _ => Ok(samples.len() / channels),
                }
            }

            fn wait(&mut self, _ms: c_int) -> Result<(), c_int> {
                Ok(())
            }

            fn recover(&mut self, error: c_int) -> Result<(), c_int> {
                if error == -EPIPE { Ok(()) } else { Err(error) }
            }
        }

        let stop = Arc::new(AtomicBool::new(false));
        let buffer_frames = AtomicUsize::new(0);
        let device = Steady {
            writes: 0,
            stop: Arc::clone(&stop),
        };
        let mut render = Quiet {
            first: None,
            last: None,
            underruns: 0,
        };
        play(
            Some((device, FORMAT)),
            FORMAT,
            || -> Option<(Steady, Format)> { unreachable!("the device is never lost") },
            |_| unreachable!("the device is never lost"),
            &mut render,
            &Shared {
                stop: &stop,
                buffer_frames: &buffer_frames,
            },
            TIMING,
            &Wall,
        );

        assert_eq!(render.underruns, 1_000);
        let (Some(first), Some(last)) = (render.first, render.last) else {
            panic!("nothing was rendered");
        };
        let counts = last.since(first);
        assert_eq!((counts.allocations, counts.deallocations), (0, 0));
    }

    /// Counts renders, and stops the loop at its `at`th.
    struct StopAt {
        renders: usize,
        at: usize,
        stop: Arc<AtomicBool>,
    }

    impl AudioRender for StopAt {
        fn render(&mut self, _out: &mut [f32], _channels: u16, _sample_rate: u32) {
            self.renders += 1;
            if self.renders == self.at {
                self.stop.store(true, Ordering::Relaxed);
            }
        }

        fn underrun(&mut self) {}
    }

    #[test]
    fn a_stop_while_there_is_no_device_ends_the_loop_at_that_period() {
        let test = Test::new("stop-lost");
        let mut render = StopAt {
            renders: 0,
            at: 5,
            stop: Arc::clone(&test.stop),
        };
        let mut tries = 0;
        play(
            None,
            MILLISECOND,
            || -> Option<(Fake, Format)> {
                tries += 1;
                None
            },
            |error| format!("{error}"),
            &mut render,
            &Shared {
                stop: &test.stop,
                buffer_frames: &test.buffer_frames,
            },
            FAST,
            &*test.clock,
        );

        // Five periods of 1 ms, before the first try at 10.
        assert_eq!((render.renders, tries), (5, 0));
        assert_eq!(test.clock.elapsed(), Duration::from_millis(5));
    }

    #[test]
    fn a_stream_s_drop_waits_for_its_thread_only_so_long() {
        let stop = Arc::new(AtomicBool::new(false));
        let (done, finished) = mpsc::channel::<()>();
        // A thread that does not look at the stop, as one held in ALSA.
        let thread = thread::spawn(move || {
            let _done = done;
            thread::sleep(Duration::from_millis(500));
        });
        let stream = Stream {
            stop: Arc::clone(&stop),
            thread: Some(thread),
            finished: Mutex::new(finished),
            buffer_frames: Arc::default(),
            sample_rate: 48_000,
            join_wait: Duration::from_millis(50),
        };
        let dropped = Instant::now();
        drop(stream);
        let took = dropped.elapsed();

        assert!(stop.load(Ordering::Relaxed));
        assert!(
            (Duration::from_millis(50)..Duration::from_millis(400)).contains(&took),
            "{took:?}"
        );
        let lines = log::tail(log::RING_CAPACITY);
        assert!(
            lines.iter().any(|line| line.contains(
                "the audio thread did not stop within 0.05 s: it is left to end with the process"
            )),
            "{lines:?}"
        );
    }

    #[test]
    fn a_refused_format_is_told_once_until_it_changes() {
        let refused = OpenFailure::code("snd_pcm_set_params", -22);
        let other = OpenFailure::message("snd_pcm_get_params", "it gave a period of no frames");
        let absent = OpenFailure::code("snd_pcm_open", -2);
        let mut last = None;
        let told: Vec<bool> = [refused, refused, other, other, absent, absent, refused]
            .into_iter()
            .map(|failure| is_new_refusal(&mut last, failure))
            .collect();
        // A device that is not there is told once, at the start or the loss.
        assert_eq!(told, [true, false, true, false, false, false, true]);
    }

    #[test]
    fn the_nominal_format_is_what_set_params_asks_for() {
        let config = AudioOutputConfig::default();
        assert_eq!(
            Format::nominal(&config),
            Format {
                sample_rate: 48_000,
                channels: 2,
                period_frames: 360,
                buffer_frames: 1_440,
            }
        );
        assert_eq!(
            Format::nominal(&config).period(),
            Duration::from_micros(7_500)
        );
    }
}

/// The tests that need libasound (feature `headless-test`), through ALSA's
/// `file` device, which writes what it is given to a file and plays
/// nothing, and an ignored one on the default device, which plays silence.
///
/// The tests of a device that does not exist, or a format the device
/// refuses, make alsa-lib print its own `ALSA lib ...` lines to stderr.
#[cfg(all(test, feature = "headless-test"))]
mod device_tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::AtomicU64;

    use nadir_core::log;

    use super::*;
    use crate::open_audio;

    /// The `n`th sample written: a pattern that repeats every 4,001.
    fn pattern(n: usize) -> f32 {
        ((n % 4_001) as f32 - 2_000.0) / 2_000.0
    }

    /// The allocations and frees of a thread so far, where the counter is.
    fn allocations() -> u64 {
        #[cfg(feature = "dev")]
        {
            let counts = crate::thread_alloc_counts();
            counts.allocations + counts.deallocations
        }
        #[cfg(not(feature = "dev"))]
        0
    }

    /// Writes [`pattern`], and says once it has written `frames` frames:
    /// at the render after, so that the period before is written too. The
    /// file device, over `null`, never makes the thread wait, so the
    /// stream holds it to the clock.
    struct Pattern {
        next: usize,
        frames: usize,
        renders: Arc<AtomicUsize>,
        done: Option<mpsc::Sender<()>>,
        /// The thread's allocations and frees at its tenth render, and
        /// since then.
        allocations: Arc<[AtomicU64; 2]>,
        panic_at: Option<usize>,
    }

    impl Pattern {
        fn new(frames: usize, done: Option<mpsc::Sender<()>>) -> Self {
            Self {
                next: 0,
                frames,
                renders: Arc::default(),
                done,
                allocations: Arc::default(),
                panic_at: None,
            }
        }
    }

    impl AudioRender for Pattern {
        fn render(&mut self, out: &mut [f32], channels: u16, sample_rate: u32) {
            assert_eq!((channels, sample_rate), (2, 48_000));
            // 30 ms in periods of a quarter of it, as `snd_pcm_set_params`
            // makes them on the file device.
            assert_eq!(out.len(), 720);
            let renders = self.renders.fetch_add(1, Ordering::Relaxed);
            assert_ne!(Some(renders), self.panic_at, "the test's render panics");
            let now = allocations();
            if renders == 9 {
                self.allocations[0].store(now, Ordering::Relaxed);
            }
            self.allocations[1].store(now, Ordering::Relaxed);
            if self.next >= self.frames * 2
                && let Some(done) = self.done.take()
            {
                // The test waits on the receiver.
                let _ = done.send(());
            }
            for sample in out.iter_mut() {
                *sample = pattern(self.next);
                self.next += 1;
            }
        }

        fn underrun(&mut self) {
            panic!("the file device never runs out");
        }
    }

    /// A file of this test's own, for the file device to write.
    fn output(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "nadir-platform-alsa-{}-{name}.raw",
            std::process::id()
        ));
        // A file left by an earlier run of this process's id.
        let _ = fs::remove_file(&path);
        path
    }

    /// The file device that writes raw samples to `path`.
    fn file_device(path: &Path) -> CString {
        let path = path.display().to_string();
        // The device's arguments end at a `'`, and it expands `%` and `\`.
        assert!(
            !path.contains(['\'', '%', '\\']),
            "the temporary folder's path {path} has a character the file device takes for \
             another; set TMPDIR to one without"
        );
        CString::new(format!("file:'{path}',raw")).expect("a temporary path has no NUL")
    }

    #[test]
    fn the_file_device_gets_every_sample_in_order() {
        let path = output("order");
        // A quarter of a second of sound, which takes about as long.
        let frames = 12_000;
        let (done, finished) = mpsc::channel();
        let render = Pattern::new(frames, Some(done));
        let allocated = Arc::clone(&render.allocations);
        let started = start(
            file_device(&path),
            AudioOutputConfig::default(),
            Box::new(render),
            TIMING,
        );
        let stream = started.unwrap_or_else(|error| panic!("{error}"));
        // 1,440 frames of buffer.
        assert_eq!(stream.latency(), Some(Duration::from_millis(30)));

        finished
            .recv_timeout(Duration::from_secs(10))
            .expect("the audio thread rendered a second of sound");
        // Joins the thread, which closes the device and flushes the file.
        drop(stream);

        let bytes = fs::read(&path).expect("the file device wrote its file");
        let _ = fs::remove_file(&path);
        let (samples, rest) = bytes.as_chunks::<4>();
        assert!(rest.is_empty(), "{} bytes", bytes.len());
        assert!(samples.len() >= frames * 2, "{} samples", samples.len());
        assert_eq!(samples.len() % 720, 0, "whole periods");
        for (n, sample) in samples.iter().enumerate() {
            let sample = f32::from_le_bytes(*sample);
            assert_eq!(sample.to_bits(), pattern(n).to_bits(), "sample {n}");
        }
        // The audio thread's own allocations, once it was playing: ALSA's
        // are the C library's, which the counter does not see. Without the
        // `dev` feature there is no counter, and both are 0.
        let [first, last] = &*allocated;
        let since = last.load(Ordering::Relaxed) - first.load(Ordering::Relaxed);
        assert_eq!(since, 0, "allocations and frees on the audio thread");
    }

    #[test]
    fn a_device_with_no_clock_is_played_in_time() {
        let render = Pattern::new(usize::MAX / 4, None);
        let renders = Arc::clone(&render.renders);
        let started = Instant::now();
        let stream = start(
            c"null".to_owned(),
            AudioOutputConfig::default(),
            Box::new(render),
            TIMING,
        )
        .unwrap_or_else(|error| panic!("{error}"));
        thread::sleep(Duration::from_millis(300));
        drop(stream);
        let elapsed = started.elapsed();

        // A period of 7.5 ms, and a buffer of four of them ahead.
        let renders = renders.load(Ordering::Relaxed);
        let in_time = (elapsed.as_secs_f64() / 0.0075) as usize + 4;
        assert!(
            (in_time / 2..=in_time + 2).contains(&renders),
            "{renders} renders in {elapsed:?}"
        );
    }

    #[test]
    fn a_device_that_does_not_open_starts_the_stream_without_one() {
        let (done, finished) = mpsc::channel();
        let render = Pattern::new(1, Some(done));
        let name = c"nadir_platform_has_no_such_device".to_owned();
        let stream = start(name, AudioOutputConfig::default(), Box::new(render), TIMING)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(stream.latency(), None);
        // It renders, in time, while it waits for a device.
        finished
            .recv_timeout(Duration::from_secs(10))
            .expect("the audio thread renders with no device");
        drop(stream);

        let lines = log::tail(log::RING_CAPACITY);
        let warned = lines.iter().any(|line| {
            line.contains(
                "no audio device: snd_pcm_open failed: No such file or directory (the ALSA \
                 device `nadir_platform_has_no_such_device`). Trying to open it again every 1 s",
            )
        });
        assert!(warned, "{lines:?}");
    }

    #[test]
    fn a_format_the_device_refuses_fails_in_its_setup() {
        let path = output("refused");
        // `open_audio` refuses it before a device sees it.
        let config = AudioOutputConfig {
            channels: 0,
            ..AudioOutputConfig::default()
        };
        let render = Pattern::new(0, None);
        let result = start(file_device(&path), config, Box::new(render), TIMING);
        let _ = fs::remove_file(&path);
        let error = match result {
            Ok(_) => panic!("the device took no channels"),
            Err(error) => error,
        };
        let PlatformError::AudioCallFailed { call, error } = error else {
            panic!("{error}");
        };
        assert_eq!(call, "snd_pcm_set_params");
        assert!(error.contains("(the ALSA device `file:"), "{error}");
    }

    #[test]
    fn a_render_that_panics_ends_the_stream_and_its_drop_returns() {
        let path = output("panic");
        let mut render = Pattern::new(usize::MAX / 4, None);
        render.panic_at = Some(3);
        let renders = Arc::clone(&render.renders);
        let stream = start(
            file_device(&path),
            AudioOutputConfig::default(),
            Box::new(render),
            TIMING,
        )
        .unwrap_or_else(|error| panic!("{error}"));
        // The thread has ended by then.
        thread::sleep(Duration::from_millis(100));
        assert_eq!(
            renders.load(Ordering::Relaxed),
            4,
            "nothing renders after the panic"
        );
        assert_eq!(stream.latency(), None, "no device plays");
        let dropped = Instant::now();
        drop(stream);
        let _ = fs::remove_file(&path);
        assert!(dropped.elapsed() < JOIN_WAIT, "{:?}", dropped.elapsed());
        // The three periods before the panic.
        let bytes = fs::read(&path).unwrap_or_default();
        assert!(bytes.len() <= 3 * 720 * 4, "{} bytes", bytes.len());
    }

    /// Silence, with a drop that takes 600 ms.
    struct SlowToDrop;

    impl AudioRender for SlowToDrop {
        fn render(&mut self, out: &mut [f32], _channels: u16, _sample_rate: u32) {
            out.fill(0.0);
        }

        fn underrun(&mut self) {}
    }

    impl Drop for SlowToDrop {
        fn drop(&mut self) {
            thread::sleep(Duration::from_millis(600));
        }
    }

    #[test]
    fn the_drop_s_limit_covers_the_render_s_own_drop() {
        let timing = Timing {
            join_wait: Duration::from_millis(100),
            ..TIMING
        };
        let stream = start(
            c"null".to_owned(),
            AudioOutputConfig::default(),
            Box::new(SlowToDrop),
            timing,
        )
        .unwrap_or_else(|error| panic!("{error}"));
        let dropped = Instant::now();
        drop(stream);
        let took = dropped.elapsed();
        assert!(took < Duration::from_millis(400), "{took:?}");
    }

    #[test]
    fn a_device_slow_to_open_is_waited_for_only_so_long() {
        // A FIFO with no reader: the file device's open waits until one
        // comes, as a sound server that does not answer would.
        let path = output("slow.fifo");
        let made = std::process::Command::new("mkfifo")
            .arg(&path)
            .status()
            .expect("mkfifo runs");
        assert!(made.success());
        let timing = Timing {
            open_wait: Duration::from_millis(100),
            ..TIMING
        };
        let started = Instant::now();
        let stream = start(
            file_device(&path),
            AudioOutputConfig::default(),
            Box::new(Pattern::new(usize::MAX / 4, None)),
            timing,
        )
        .unwrap_or_else(|error| panic!("{error}"));
        let took = started.elapsed();
        assert!(took < Duration::from_millis(1_000), "{took:?}");
        assert_eq!(stream.latency(), None);
        let lines = log::tail(log::RING_CAPACITY);
        assert!(
            lines.iter().any(|line| line.contains(
                "the audio device is slow to open: going on without it for now, after 0.1 s"
            )),
            "{lines:?}"
        );

        // A reader comes: the device opens, and plays.
        let reader = {
            let path = path.clone();
            thread::spawn(move || fs::read(&path).map(|bytes| bytes.len()))
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        while stream.latency().is_none() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(stream.latency(), Some(Duration::from_millis(30)));
        thread::sleep(Duration::from_millis(50));
        // Closing the device ends the reader's file.
        drop(stream);
        let read = reader.join().expect("the reader does not panic");
        let _ = fs::remove_file(&path);
        assert!(read.as_ref().is_ok_and(|&bytes| bytes > 0), "{read:?}");
    }

    #[test]
    fn quietly_puts_the_thread_s_own_handler_back() {
        unsafe extern "C" fn marker(
            _file: *const c_char,
            _line: c_int,
            _func: *const c_char,
            _err: c_int,
            _fmt: *const c_char,
            _arg: *mut c_void,
        ) {
        }
        type Handler = nadir_sys::linux::alsa::snd_local_error_handler_t;
        let address = |handler: Handler| handler.map(|handler| handler as usize);

        let alsa = alsa().unwrap_or_else(|error| panic!("{error}"));
        let set_local = alsa
            .set_local
            .expect("libasound 1.2 has snd_lib_error_set_local");
        // SAFETY: `marker` has the handler's type and lives for ever.
        let before = unsafe { set_local(Some(marker)) };
        let during = quietly(alsa, || {
            // SAFETY: as above; this puts back what was set.
            let set = unsafe { set_local(None) };
            // SAFETY: as above.
            unsafe { set_local(set) };
            set
        });
        // SAFETY: as above.
        let after = unsafe { set_local(before) };

        let marker = address(Some(marker));
        assert!(
            during.is_some() && address(during) != marker,
            "quiet's handler"
        );
        assert_eq!(address(after), marker, "the thread's own handler is back");
    }

    #[test]
    fn a_real_device_s_errors_come_back_negative() {
        let alsa = alsa().unwrap_or_else(|error| panic!("{error}"));
        let mut pcm = ptr::null_mut();
        // SAFETY: `pcm` is a place for the handle, and the name ends with a
        // NUL.
        let result = unsafe {
            (alsa.asound.snd_pcm_open)(
                &raw mut pcm,
                c"null".as_ptr(),
                SND_PCM_STREAM_PLAYBACK,
                SND_PCM_NONBLOCK,
            )
        };
        assert_eq!(result, 0);
        let mut pcm = Pcm {
            alsa,
            pcm: NonNull::new(pcm).expect("an open PCM"),
        };
        // Not set up: a write fails, and so does a recovery from an error
        // `snd_pcm_recover` does not handle (-EBADFD), with its code.
        let written = pcm.write(&[0.0; 8], 2);
        assert!(matches!(written, Err(error) if error < 0), "{written:?}");
        assert_eq!(pcm.recover(-77), Err(-77));
    }

    /// Silence, with one period late by three buffers.
    struct Late {
        renders: Arc<AtomicUsize>,
        /// The render after which each underrun came.
        underruns: Arc<Mutex<Vec<usize>>>,
        late_by: Duration,
    }

    impl AudioRender for Late {
        fn render(&mut self, out: &mut [f32], _channels: u16, _sample_rate: u32) {
            out.fill(0.0);
            if self.renders.fetch_add(1, Ordering::Relaxed) == 20 {
                thread::sleep(self.late_by);
            }
        }

        fn underrun(&mut self) {
            let after = self.renders.load(Ordering::Relaxed);
            self.underruns
                .lock()
                .expect("no test panics while holding it")
                .push(after);
        }
    }

    #[test]
    #[ignore = "needs an audio device, to which it plays a second of silence"]
    fn the_default_device_reports_the_underrun_of_a_late_period_and_plays_on() {
        let renders = Arc::new(AtomicUsize::new(0));
        let underruns = Arc::new(Mutex::new(Vec::new()));
        let config = AudioOutputConfig::default();
        let render = Late {
            renders: Arc::clone(&renders),
            underruns: Arc::clone(&underruns),
            // Three buffers of 30 ms, and some.
            late_by: Duration::from_millis(3 * 30 + 50),
        };
        let stream =
            open_audio(&config, Box::new(render)).unwrap_or_else(|error| panic!("{error}"));
        let latency = stream.latency().expect("the default device opened");
        assert!(latency <= Duration::from_millis(100), "{latency:?}");

        let deadline = Instant::now() + Duration::from_secs(10);
        while renders.load(Ordering::Relaxed) < 60 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        drop(stream);
        assert!(renders.load(Ordering::Relaxed) >= 60, "it stopped playing");
        let underruns = underruns
            .lock()
            .expect("no test panics while holding it")
            .clone();
        // The late period was the 21st render: the underrun comes in its
        // write, before the 22nd.
        assert!(
            underruns.contains(&21),
            "underruns after renders {underruns:?}"
        );
    }
}
