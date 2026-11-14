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

//! Timers: the GPU's time over a pass, and the order their calls keep.

use std::env;
use std::time::{Duration, Instant};

use nadir_core::LinearColor;
use nadir_gfx::{BlendMode, GfxError, ResourceKind, TextureFormat, TimerId};
use nadir_platform::GpuSurface;

use crate::common::{EXPECT_RENDERER, FULL, Gpu, color_pipeline};

/// A device whose driver has timer queries. Where a renderer is asked for
/// (llvmpipe in CI), it must have them.
fn timed_gpu() -> Option<Gpu> {
    let gpu = Gpu::new()?;
    if gpu.device.caps().timer_queries {
        return Some(gpu);
    }
    let expected = env::var(EXPECT_RENDERER).unwrap_or_default();
    assert!(
        expected.is_empty(),
        "`{}` has no timer queries",
        gpu.device.caps().renderer
    );
    None
}

/// The timer's result, asked for until it comes, which takes no longer
/// than the GPU's work.
fn wait_for(gpu: &mut Gpu, timer: TimerId) -> Duration {
    let started = Instant::now();
    loop {
        if let Some(elapsed) = gpu.device.timer_result(timer).expect("a timer's result") {
            return elapsed;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the timer's result did not come in 10 s"
        );
        std::thread::yield_now();
    }
}

#[test]
fn a_timer_measures_the_gpus_time_over_a_pass() {
    let Some(mut gpu) = timed_gpu() else { return };
    let target = gpu.target(512, 512, TextureFormat::Rgba8);
    let pipeline = color_pipeline(&mut gpu, TextureFormat::Rgba8, BlendMode::Alpha);
    let quad = gpu.vertices(&FULL);
    let timer = gpu.device.create_timer().unwrap();
    assert_eq!(gpu.device.timer_result(timer), Ok(None), "it never ran");
    let layers = |gpu: &mut Gpu| {
        let clear = Some(LinearColor::new(0.0, 0.0, 0.0, 1.0));
        let mut pass = gpu.device.begin_pass(target.into(), clear).unwrap();
        pass.set_pipeline(pipeline).unwrap();
        pass.set_vertex_buffer(0, quad, 0).unwrap();
        // Sixty-four blended layers over a quarter of a million pixels:
        // work that takes the GPU some time.
        for _ in 0..64 {
            pass.set_uniforms(0, &[0.2f32, 0.4, 0.6, 0.1]).unwrap();
            pass.draw(0..4, 0..1).unwrap();
        }
    };
    // llvmpipe (Mesa 26.1) times the context's first drawing at hours: the
    // timed frame is the second.
    gpu.begin();
    layers(&mut gpu);
    gpu.end();

    let began = Instant::now();
    gpu.begin();
    gpu.device.begin_timer(timer).unwrap();
    assert_eq!(gpu.device.timer_result(timer), Ok(None), "it runs");
    layers(&mut gpu);
    gpu.device.end_timer().unwrap();
    gpu.end();
    let elapsed = wait_for(&mut gpu, timer);
    // The GPU did the work after the timer began, and before its result
    // came.
    let passed = began.elapsed();
    assert!(
        Duration::ZERO < elapsed && elapsed <= passed,
        "the pass took {elapsed:?} of the {passed:?} that passed"
    );
    // The result stays until the timer runs again, which drops it.
    assert_eq!(gpu.device.timer_result(timer), Ok(Some(elapsed)));
    gpu.begin();
    gpu.device.begin_timer(timer).unwrap();
    assert_eq!(gpu.device.timer_result(timer), Ok(None));
    gpu.device.end_timer().unwrap();
    gpu.end();
    wait_for(&mut gpu, timer);
}

#[test]
fn timers_run_one_at_a_time_inside_a_frame() {
    let Some(mut gpu) = timed_gpu() else { return };
    let [first, second] = [(); 2].map(|()| gpu.device.create_timer().unwrap());
    assert_eq!(
        gpu.device.begin_timer(first),
        Err(GfxError::FrameOrder("a timer began outside a frame"))
    );
    gpu.begin();
    assert_eq!(
        gpu.device.end_timer(),
        Err(GfxError::FrameOrder("end_timer came while no timer ran"))
    );
    gpu.device.begin_timer(first).unwrap();
    assert_eq!(
        gpu.device.begin_timer(second),
        Err(GfxError::FrameOrder("a timer began while another ran"))
    );
    gpu.device.end_timer().unwrap();
    // One after the other, in one frame.
    gpu.device.begin_timer(second).unwrap();
    gpu.device.end_timer().unwrap();
    gpu.end();
    wait_for(&mut gpu, first);
    wait_for(&mut gpu, second);
}

#[test]
fn a_frame_that_ends_while_a_timer_runs_stops_it_and_fails() {
    let Some(mut gpu) = timed_gpu() else { return };
    let timer = gpu.device.create_timer().unwrap();
    gpu.begin();
    gpu.device.begin_timer(timer).unwrap();
    assert_eq!(
        gpu.device.end_frame(GpuSurface::Gl(&gpu.gl)),
        Err(GfxError::FrameOrder("end_frame came while a timer ran"))
    );
    // The frame is over, and the timer stopped: it has a result, and the
    // next frame can run it again.
    wait_for(&mut gpu, timer);
    gpu.begin();
    gpu.device.begin_timer(timer).unwrap();
    gpu.device.end_timer().unwrap();
    gpu.end();
    wait_for(&mut gpu, timer);
}

#[test]
fn a_destroyed_timer_stops_and_its_id_goes_stale() {
    let Some(mut gpu) = timed_gpu() else { return };
    let [running, other] = [(); 2].map(|()| gpu.device.create_timer().unwrap());
    gpu.begin();
    gpu.device.begin_timer(running).unwrap();
    gpu.device.destroy(running);
    // It no longer runs, so another can.
    assert_eq!(
        gpu.device.end_timer(),
        Err(GfxError::FrameOrder("end_timer came while no timer ran"))
    );
    gpu.device.begin_timer(other).unwrap();
    gpu.device.end_timer().unwrap();
    gpu.end();
    wait_for(&mut gpu, other);
    let stale = Err(GfxError::StaleHandle(ResourceKind::Timer));
    assert_eq!(gpu.device.timer_result(running), stale);
    gpu.begin();
    assert_eq!(gpu.device.begin_timer(running), stale.map(|_| ()));
    gpu.end();
}
