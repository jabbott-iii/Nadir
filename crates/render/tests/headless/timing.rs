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

//! The GPU's time over the renderer's passes.

use std::env;
use std::time::Duration;

use nadir_assets::Handle;
use nadir_core::{UVec2, Vec2};
use nadir_gfx::{GfxError, ResourceKind, TextureFormat};
use nadir_render::{Output, RenderError, RenderFrame, SpriteDraw, Texture};

use crate::common::{EXPECT_RENDERER, Stage};

const OUTPUT: UVec2 = UVec2::new(640, 360);

/// A stage whose device has timer queries. Where a renderer is asked for
/// (llvmpipe in CI), it must have them.
fn timed_stage(test: &str) -> Option<Stage> {
    let stage = Stage::new(test, UVec2::new(320, 180))?;
    if stage.device.caps().timer_queries {
        return Some(stage);
    }
    let expected = env::var(EXPECT_RENDERER).unwrap_or_default();
    assert!(
        expected.is_empty(),
        "`{}` has no timer queries",
        stage.device.caps().renderer
    );
    None
}

/// A frame of 200 sprites of `texture`.
fn crowd(texture: Handle<Texture>) -> RenderFrame {
    let mut frame = RenderFrame::new();
    for i in 0..200u16 {
        let position = Vec2::new(f32::from(i % 20) * 16.0, f32::from(i / 20) * 18.0 + 16.0);
        frame
            .sprites
            .push(SpriteDraw::new(texture, position, Vec2::new(16.0, 16.0)));
    }
    frame
}

/// Renders `frame` into a render target, in a frame of the device's, then
/// reads the target back, which waits for the GPU to finish the frame.
fn render(stage: &mut Stage, frame: &RenderFrame) -> Result<(), RenderError> {
    let target = stage
        .device
        .create_render_target(OUTPUT, TextureFormat::Rgba8Srgb)
        .expect("an output");
    stage.device.begin_offscreen_frame().expect("a frame");
    let output = Output {
        target: target.into(),
        size: OUTPUT,
    };
    let rendered = stage
        .renderer
        .render(
            &mut stage.device,
            &stage.server,
            &stage.tiles,
            frame,
            output,
        )
        .map(drop);
    stage
        .device
        .end_offscreen_frame()
        .expect("the end of the frame");
    stage.device.read_pixels(target).expect("the output");
    stage.device.destroy(target);
    rendered
}

#[test]
fn each_render_is_timed_and_read_back_two_renders_later() {
    let Some(mut stage) = timed_stage("gpu_times") else {
        return;
    };
    let texture = stage.image("sprites/white.qoi", 1, 1, &[[255; 4]]);
    stage.finish();
    let frame = crowd(texture);

    // Off at first. This frame is also the context's first drawing, which
    // llvmpipe times at hours.
    assert!(!stage.renderer.gpu_timing());
    render(&mut stage, &frame).unwrap();
    assert_eq!(stage.renderer.gpu_times(), []);

    stage.renderer.set_gpu_timing(true);
    for render_index in 0..10 {
        render(&mut stage, &frame).unwrap();
        let times = stage.renderer.gpu_times();
        if render_index < 2 {
            assert_eq!(times, [], "render {render_index} has nothing to read");
            continue;
        }
        // Each render waited for the GPU, so the one two before is there,
        // and only it.
        assert_eq!(times.len(), 1, "render {render_index}: {times:?}");
        let times = times[0];
        assert_eq!(times.renders_ago, 2);
        assert!(times.world > Duration::ZERO, "{times:?}");
        assert!(times.output > Duration::ZERO, "{times:?}");
        assert_eq!(times.total(), times.world + times.output);
    }

    stage.renderer.set_gpu_timing(false);
    render(&mut stage, &frame).unwrap();
    assert_eq!(stage.renderer.gpu_times(), [], "off again");
}

#[test]
fn a_render_that_fails_stops_its_timer() {
    let Some(mut stage) = timed_stage("gpu_times_after_a_failure") else {
        return;
    };
    let texture = stage.image("sprites/white.qoi", 1, 1, &[[255; 4]]);
    stage.finish();
    let frame = crowd(texture);
    render(&mut stage, &frame).unwrap();
    stage.renderer.set_gpu_timing(true);
    render(&mut stage, &frame).unwrap();

    // The texture goes behind the asset server's back: the world pass
    // fails with its timer running.
    let id = stage.server.get(texture).expect("the texture").id();
    stage.device.destroy(id);
    assert_eq!(
        render(&mut stage, &frame),
        Err(RenderError::Gfx(GfxError::StaleHandle(
            ResourceKind::Texture
        )))
    );

    // The timer stopped, so the next renders run theirs. The first timed
    // render is read back, and the failed one is not waited for.
    let empty = RenderFrame::new();
    render(&mut stage, &empty).unwrap();
    assert_eq!(stage.renderer.gpu_times().len(), 1);
    assert_eq!(stage.renderer.gpu_times()[0].renders_ago, 2);
    render(&mut stage, &empty).unwrap();
    assert_eq!(stage.renderer.gpu_times(), [], "the failed render");
    for _ in 0..4 {
        render(&mut stage, &empty).unwrap();
        assert_eq!(stage.renderer.gpu_times().len(), 1);
    }
}
