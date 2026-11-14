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

//! The GPU's time over the renderer's passes: a ring of timers, each
//! render's read back two renders later, so that reading never waits for
//! the GPU.

use core::time::Duration;
use std::time::Instant;

use nadir_core::log::warn;
use nadir_gfx::{Device, GfxError, TimerId};

/// The GPU's time over the passes of one call of
/// [`Renderer::render`](crate::Renderer::render), as the device's timers
/// measured it. [`Renderer::gpu_times`](crate::Renderer::gpu_times) gives
/// them.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GpuTimes {
    /// How many calls of `render` before the one that read these times
    /// back the timed call was: 2 while the GPU keeps up, more when it
    /// falls behind.
    pub renders_ago: u32,
    /// The world pass: the sprites, texts and tile layers drawn into the
    /// virtual image.
    pub world: Duration,
    /// The output pass: the virtual image scaled onto the output, then the
    /// UI drawn over it.
    pub output: Duration,
}

impl GpuTimes {
    /// The two passes' times together.
    pub fn total(&self) -> Duration {
        self.world + self.output
    }
}

/// The passes a render times: the world's, then the output's.
pub(crate) const PASSES: usize = 2;

/// The renders whose times can wait for the GPU at once. A render whose
/// slot still waits is not timed.
const SLOTS: usize = 4;

/// The renders after a timed one before its times are first read: by then
/// the GPU has nearly always done its work, so reading costs one check a
/// timer.
const LATENCY: u64 = 2;

/// What the ring needs of a device: timers to make and read. The ring's
/// tests stand in for the GPU with one of their own.
pub(crate) trait Timers {
    /// A timer.
    type Id: Copy;
    /// Makes a timer.
    fn create(&mut self) -> Result<Self::Id, GfxError>;
    /// The time the timer measured, once the GPU has done the work: `None`
    /// until then. It never waits.
    fn result(&mut self, timer: Self::Id) -> Result<Option<Duration>, GfxError>;
}

impl Timers for Device {
    type Id = TimerId;

    fn create(&mut self) -> Result<TimerId, GfxError> {
        self.create_timer()
    }

    fn result(&mut self, timer: TimerId) -> Result<Option<Duration>, GfxError> {
        self.timer_result(timer)
    }
}

/// A slot of the ring: the timers of one render's passes.
#[derive(Clone, Copy)]
struct Slot<T> {
    /// Made at the slot's first render.
    timers: Option<[T; PASSES]>,
    /// The render the timers timed, and when it started, until its times
    /// are read.
    waiting: Option<(u64, Instant)>,
}

/// The renders being timed, and the times read back.
pub(crate) struct TimerRing<T> {
    slots: [Slot<T>; SLOTS],
    /// The renders started so far.
    renders: u64,
    /// The slot of the render started last, while it runs: it waits for
    /// the GPU only once the render ends.
    current: Option<(usize, u64, Instant)>,
    /// The times the last render read back, oldest first.
    read: Vec<GpuTimes>,
    /// Whether a time the driver got wrong was logged.
    warned: bool,
}

impl<T: Copy> TimerRing<T> {
    pub(crate) fn new() -> Self {
        Self {
            slots: [Slot {
                timers: None,
                waiting: None,
            }; SLOTS],
            renders: 0,
            current: None,
            // Each slot's times read at once, at most: a full ring
            // allocates nothing more.
            read: Vec::with_capacity(SLOTS),
            warned: false,
        }
    }

    /// The times the last render read back, oldest first.
    pub(crate) fn read(&self) -> &[GpuTimes] {
        &self.read
    }

    /// Starts a render at `now`: reads back the times of the renders at
    /// least [`LATENCY`] renders before it whose timers all have their
    /// results, and gives the timers to time this render's passes with,
    /// made at the slot's first use: none while its slot still waits for
    /// the GPU.
    ///
    /// A time longer than the time that has passed since its render
    /// started cannot be right, and is dropped.
    pub(crate) fn begin_render(
        &mut self,
        timers: &mut impl Timers<Id = T>,
        now: Instant,
    ) -> Result<Option<[T; PASSES]>, GfxError> {
        let render = self.renders;
        self.renders += 1;
        self.current = None;
        self.read.clear();
        // The waiting slots, oldest first; an empty one sorts last.
        let mut order = [(u64::MAX, 0); SLOTS];
        for ((entry, slot), index) in order.iter_mut().zip(&self.slots).zip(0..) {
            if let Some((timed, _)) = slot.waiting {
                *entry = (timed, index);
            }
        }
        order.sort_unstable();
        for (timed, index) in order {
            if timed == u64::MAX || render - timed < LATENCY {
                continue;
            }
            let slot = &mut self.slots[index];
            let (Some(ids), Some((_, started))) = (slot.timers, slot.waiting) else {
                continue;
            };
            let mut times = [Duration::ZERO; PASSES];
            let mut ready = true;
            for (time, id) in times.iter_mut().zip(ids) {
                match timers.result(id)? {
                    Some(result) => *time = result,
                    None => {
                        ready = false;
                        break;
                    }
                }
            }
            if !ready {
                continue;
            }
            slot.waiting = None;
            let [world, output] = times;
            let passed = now.saturating_duration_since(started);
            if world + output > passed {
                if !self.warned {
                    self.warned = true;
                    warn!(
                        "the GPU's timers gave {:?} for a render {passed:?} ago: the driver \
                         timed it wrong, and its times are dropped (logged once)",
                        world + output
                    );
                }
                continue;
            }
            self.read.push(GpuTimes {
                renders_ago: u32::try_from(render - timed).unwrap_or(u32::MAX),
                world,
                output,
            });
        }

        let index = (render % SLOTS as u64) as usize;
        let slot = &mut self.slots[index];
        if slot.waiting.is_some() {
            return Ok(None);
        }
        let ids = match slot.timers {
            Some(ids) => ids,
            None => {
                let ids = [timers.create()?, timers.create()?];
                slot.timers = Some(ids);
                ids
            }
        };
        self.current = Some((index, render, now));
        Ok(Some(ids))
    }

    /// Ends the render started last, which timed its passes: its slot
    /// waits for the GPU. A render that failed before its end is not
    /// waited for, its timers free for the next.
    pub(crate) fn end_render(&mut self) {
        if let Some((index, render, started)) = self.current.take() {
            self.slots[index].waiting = Some((render, started));
        }
    }

    /// Forgets the renders that wait and the times read, keeping the
    /// timers.
    pub(crate) fn reset(&mut self) {
        for slot in &mut self.slots {
            slot.waiting = None;
        }
        self.current = None;
        self.read.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A GPU of the test's: timers numbered from 0, whose results come when
    /// the test says.
    #[derive(Default)]
    struct FakeGpu {
        made: u32,
        results: Vec<Option<Duration>>,
        /// The timers asked for a result, in order.
        asked: Vec<u32>,
    }

    impl FakeGpu {
        /// The GPU has done a render's work: its timers have results.
        fn finish(&mut self, [world, output]: [u32; PASSES], times: [Duration; PASSES]) {
            self.results[world as usize] = Some(times[0]);
            self.results[output as usize] = Some(times[1]);
        }
    }

    impl Timers for FakeGpu {
        type Id = u32;

        fn create(&mut self) -> Result<u32, GfxError> {
            self.results.push(None);
            self.made += 1;
            Ok(self.made - 1)
        }

        fn result(&mut self, timer: u32) -> Result<Option<Duration>, GfxError> {
            self.asked.push(timer);
            Ok(self.results[timer as usize])
        }
    }

    const FRAME: Duration = Duration::from_millis(16);

    fn ms(ms: u64) -> Duration {
        Duration::from_millis(ms)
    }

    /// One render at `now`: begins it, and ends it.
    fn render(ring: &mut TimerRing<u32>, gpu: &mut FakeGpu, now: Instant) -> Option<[u32; 2]> {
        let timers = ring.begin_render(gpu, now).unwrap();
        ring.end_render();
        timers
    }

    #[test]
    fn a_render_is_read_back_two_renders_later() {
        let (mut ring, mut gpu) = (TimerRing::new(), FakeGpu::default());
        let start = Instant::now();
        for frame in 0..12u32 {
            let now = start + FRAME * frame;
            let timers = render(&mut ring, &mut gpu, now).expect("every render is timed");
            // The GPU keeps up: its work is done before the next render.
            gpu.finish(timers, [ms(u64::from(frame)), ms(1)]);
            if frame < 2 {
                assert_eq!(ring.read(), [], "render {frame}");
            } else {
                let times = GpuTimes {
                    renders_ago: 2,
                    world: ms(u64::from(frame - 2)),
                    output: ms(1),
                };
                assert_eq!(ring.read(), [times], "render {frame}");
                assert_eq!(times.total(), ms(u64::from(frame - 1)));
            }
        }
        // Four slots of two timers, made once each.
        assert_eq!(gpu.made, 8);
    }

    #[test]
    fn no_time_is_read_before_its_result_is_there() {
        let (mut ring, mut gpu) = (TimerRing::new(), FakeGpu::default());
        let start = Instant::now();
        let first = render(&mut ring, &mut gpu, start).unwrap();
        // The GPU falls behind: the first render's work is not done until
        // the fourth render.
        let mut timed = Vec::new();
        for frame in 1..4 {
            timed.push(render(&mut ring, &mut gpu, start + FRAME * frame).unwrap());
            assert_eq!(ring.read(), [], "render {frame}");
        }
        // A render is asked for two renders on, and a slot only as far as
        // its first timer without a result: the third render asks for the
        // first's world timer, and the fourth for it and the second's.
        assert_eq!(gpu.asked, [first[0], first[0], timed[0][0]]);
        // Its slot still waits: the fifth render, which would take it, is
        // not timed.
        assert_eq!(render(&mut ring, &mut gpu, start + FRAME * 4), None);
        assert_eq!(ring.read(), []);
        gpu.finish(first, [ms(5), ms(2)]);
        for timers in timed {
            gpu.finish(timers, [ms(3), ms(1)]);
        }
        // The sixth reads the first, five renders on, then the second to the
        // fourth, oldest first, and takes the second's slot.
        assert!(render(&mut ring, &mut gpu, start + FRAME * 5).is_some());
        let ago: Vec<u32> = ring.read().iter().map(|times| times.renders_ago).collect();
        assert_eq!(ago, [5, 4, 3, 2]);
        assert_eq!(
            (ring.read()[0].world, ring.read()[0].output),
            (ms(5), ms(2))
        );
    }

    #[test]
    fn times_that_come_together_come_oldest_first() {
        let (mut ring, mut gpu) = (TimerRing::new(), FakeGpu::default());
        let start = Instant::now();
        let timers: Vec<_> = (0..3)
            .map(|frame| render(&mut ring, &mut gpu, start + FRAME * frame).unwrap())
            .collect();
        // Done in reverse: the order read is still the renders' order.
        for (timers, world) in timers.iter().zip([1, 2, 3]).rev() {
            gpu.finish(*timers, [ms(world), ms(0)]);
        }
        render(&mut ring, &mut gpu, start + FRAME * 3);
        let read: Vec<(u32, Duration)> = ring
            .read()
            .iter()
            .map(|times| (times.renders_ago, times.world))
            .collect();
        // The third render is one render old: not read yet.
        assert_eq!(read, [(3, ms(1)), (2, ms(2))]);
    }

    #[test]
    fn a_time_longer_than_the_time_that_passed_is_dropped() {
        let (mut ring, mut gpu) = (TimerRing::new(), FakeGpu::default());
        let start = Instant::now();
        let wrong = render(&mut ring, &mut gpu, start).unwrap();
        // llvmpipe gives hours for the context's first drawing.
        gpu.finish(wrong, [Duration::from_secs(43_411), ms(1)]);
        let right = render(&mut ring, &mut gpu, start + FRAME).unwrap();
        gpu.finish(right, [ms(30), ms(1)]);
        render(&mut ring, &mut gpu, start + FRAME * 2);
        assert_eq!(ring.read(), [], "hours do not pass in two frames");
        // 31 ms of work cannot have taken place in the 32 ms since its
        // render started, so it is read.
        render(&mut ring, &mut gpu, start + FRAME * 3);
        assert_eq!(ring.read().len(), 1);
        assert_eq!(ring.read()[0].total(), ms(31));
    }

    #[test]
    fn a_render_that_did_not_end_is_not_waited_for() {
        let (mut ring, mut gpu) = (TimerRing::new(), FakeGpu::default());
        let start = Instant::now();
        // A render that fails between its start and its end.
        let failed = ring.begin_render(&mut gpu, start).unwrap().unwrap();
        for frame in 1..4 {
            render(&mut ring, &mut gpu, start + FRAME * frame);
        }
        assert!(
            !gpu.asked.iter().any(|timer| failed.contains(timer)),
            "its timers, perhaps never run, are not asked for"
        );
        // Its slot is free for the next render to take.
        assert_eq!(render(&mut ring, &mut gpu, start + FRAME * 4), Some(failed));
    }

    #[test]
    fn a_reset_forgets_the_renders_that_wait_and_keeps_the_timers() {
        let (mut ring, mut gpu) = (TimerRing::new(), FakeGpu::default());
        let start = Instant::now();
        for frame in 0..4 {
            let timers = render(&mut ring, &mut gpu, start + FRAME * frame).unwrap();
            gpu.finish(timers, [ms(1), ms(1)]);
        }
        assert_eq!(ring.read().len(), 1);
        ring.reset();
        assert_eq!(ring.read(), []);
        gpu.asked.clear();
        render(&mut ring, &mut gpu, start + FRAME * 4);
        render(&mut ring, &mut gpu, start + FRAME * 5);
        assert_eq!(gpu.asked, [], "nothing waits");
        assert_eq!(gpu.made, 8, "no timer made again");
    }
}
