//! Devices, contexts, frames and passes.

use std::env;
use std::thread;

use nadir_core::{LinearColor, UVec2};
use nadir_gfx::{
    BackendKind, BlendMode, BufferDesc, BufferKind, BufferUsage, Device, GfxError, PassTarget,
    TextureDesc, TextureFormat,
};
use nadir_platform::GpuSurface;

use crate::common::{EXPECT_RENDERER, FULL, Gpu, color_pipeline, headless};

#[test]
fn the_caps_say_what_the_device_runs_on() {
    let Some(gpu) = Gpu::new() else { return };
    let caps = gpu.device.caps();
    assert_eq!(caps.backend, BackendKind::OpenGl);
    assert_eq!(caps.backend.name(), "OpenGL");
    assert!(!caps.vendor.is_empty());
    assert!(!caps.renderer.is_empty());
    assert!(!caps.version.is_empty());
    assert!(caps.max_texture_size >= 1024, "{}", caps.max_texture_size);
    let expected = env::var(EXPECT_RENDERER).unwrap_or_default();
    assert!(
        caps.renderer.contains(&expected),
        "the renderer is `{}`, and {EXPECT_RENDERER} asks for `{expected}`",
        caps.renderer
    );
}

#[test]
fn a_thread_has_one_device_at_a_time() {
    let Some(gl) = headless() else { return };
    let first = Device::new(GpuSurface::Gl(&gl)).unwrap();
    assert_eq!(
        Device::new(GpuSurface::Gl(&gl)).err(),
        Some(GfxError::DeviceExists)
    );
    drop(first);
    assert!(Device::new(GpuSurface::Gl(&gl)).is_ok());
}

#[test]
fn a_device_whose_context_is_gone_does_nothing_more() {
    let Some(gl) = headless() else { return };
    let mut device = Device::new(GpuSurface::Gl(&gl)).unwrap();
    let target = device
        .create_render_target(UVec2::new(2, 2), TextureFormat::Rgba8)
        .unwrap();
    device.begin_frame(GpuSurface::Gl(&gl)).unwrap();
    {
        let mut pass = device.begin_pass(target.into(), None).unwrap();
        // Where `HeadlessGl` has no destructor yet (Windows and macOS, which
        // skip this test), dropping it does nothing.
        #[allow(clippy::drop_non_drop)]
        drop(gl);
        // libglvnd would quietly ignore GL calls with no context, so only
        // the error shows that none are made.
        assert_eq!(pass.draw(0..3, 0..1), Err(GfxError::ContextLost));
    }
    let lost = Err(GfxError::ContextLost);
    let desc = TextureDesc {
        label: "late",
        size: UVec2::new(1, 1),
        format: TextureFormat::Rgba8,
    };
    assert_eq!(device.create_texture(&desc, None).map(drop), lost);
    let buffer = BufferDesc {
        label: "late",
        kind: BufferKind::Vertex,
        usage: BufferUsage::Static,
        size: 4,
    };
    assert_eq!(device.create_buffer(&buffer).map(drop), lost);
    assert_eq!(device.read_pixels(target).map(drop), lost);
    assert_eq!(
        device
            .create_render_target(UVec2::new(1, 1), TextureFormat::Rgba8)
            .map(drop),
        lost
    );
    // A new context on the thread does not bring the device back.
    let Some(next) = headless() else { return };
    assert_eq!(device.create_texture(&desc, None).map(drop), lost);
    assert_eq!(
        device.begin_frame(GpuSurface::Gl(&next)),
        Err(GfxError::WrongSurface)
    );
    device.destroy(target);
    // The new context can have a device of its own, while the old device
    // still exists, and only one.
    let current = Device::new(GpuSurface::Gl(&next)).unwrap();
    assert_eq!(
        Device::new(GpuSurface::Gl(&next)).err(),
        Some(GfxError::DeviceExists)
    );
    // Dropping the old device leaves the new one's claim alone.
    drop(device);
    assert_eq!(
        Device::new(GpuSurface::Gl(&next)).err(),
        Some(GfxError::DeviceExists)
    );
    drop(current);
    assert!(Device::new(GpuSurface::Gl(&next)).is_ok());
}

#[test]
fn frames_and_passes_come_in_order() {
    let Some(mut gpu) = Gpu::new() else { return };
    let surface = || GpuSurface::Gl(&gpu.gl);
    assert!(matches!(
        gpu.device.end_frame(surface()),
        Err(GfxError::FrameOrder(_))
    ));
    assert!(matches!(
        gpu.device.begin_pass(PassTarget::Surface, None).map(drop),
        Err(GfxError::FrameOrder(_))
    ));
    gpu.device.begin_frame(surface()).unwrap();
    assert!(matches!(
        gpu.device.begin_frame(surface()),
        Err(GfxError::FrameOrder(_))
    ));
    gpu.device.end_frame(surface()).unwrap();
    for _ in 0..3 {
        gpu.device.begin_frame(surface()).unwrap();
        gpu.device.end_frame(surface()).unwrap();
    }
}

#[test]
fn a_surface_with_no_pixels_gives_a_pass_that_draws_nothing() {
    let Some(mut gpu) = Gpu::new() else { return };
    // The surface's format is sRGB.
    let pipeline = color_pipeline(&mut gpu, TextureFormat::Rgba8Srgb, BlendMode::Opaque);
    let quad = gpu.vertices(&FULL);
    gpu.begin();
    {
        let clear = Some(LinearColor::new(0.2, 0.4, 0.6, 1.0));
        let mut pass = gpu.device.begin_pass(PassTarget::Surface, clear).unwrap();
        assert_eq!(pass.draw(0..0, 0..1), Ok(()));
        // The checks still apply.
        assert_eq!(pass.draw(0..3, 0..1), Err(GfxError::NoPipeline));
        pass.set_pipeline(pipeline).unwrap();
        pass.set_vertex_buffer(0, quad, 0).unwrap();
        pass.set_uniforms(0, &[0.2f32, 0.4, 0.6, 1.0]).unwrap();
        // A draw that passes them draws nothing, as there is nothing to draw
        // into.
        assert_eq!(pass.draw(0..4, 0..1), Ok(()));
    }
    // In debug builds every call is checked, so a call on the missing
    // default framebuffer would have failed above or here.
    gpu.end();
}

#[test]
fn devices_on_several_threads_each_draw_their_own() {
    thread::scope(|scope| {
        for i in 0..4u8 {
            scope.spawn(move || {
                for round in 0..3u8 {
                    let Some(mut gpu) = Gpu::new() else { return };
                    let target = gpu.target(2, 2, TextureFormat::Rgba8);
                    // Fifths of 255: 0.2 is 51, exactly.
                    let level = f32::from((i + round) % 6) * 0.2;
                    gpu.clear(target, [level, 0.0, 0.0, 1.0]);
                    let expected = (i + round) % 6 * 51;
                    assert!(gpu.read(target).iter().all(|&p| p == [expected, 0, 0, 255]));
                }
            });
        }
    });
}
