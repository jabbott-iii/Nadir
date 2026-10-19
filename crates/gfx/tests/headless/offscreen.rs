//! A device with an offscreen context of its own, from
//! `Device::new_headless`, as crates that may not use `nadir-platform` make
//! one.

use std::env;

use nadir_core::{LinearColor, UVec2};
use nadir_gfx::{Device, GfxError, TextureFormat};
use nadir_platform::PlatformError;

use crate::common::{EXPECT_RENDERER, Gpu};

/// A headless device, or `None` where this system has no offscreen context
/// yet and no renderer is asked for. It must be the renderer asked for.
fn headless_device() -> Option<Device> {
    let expected = env::var(EXPECT_RENDERER).unwrap_or_default();
    match Device::new_headless() {
        Ok(device) => {
            let renderer = &device.caps().renderer;
            assert!(
                renderer.contains(&expected),
                "the renderer is `{renderer}`, not `{expected}`"
            );
            Some(device)
        }
        Err(GfxError::NoOffscreenContext) if expected.is_empty() => None,
        Err(error) => panic!("no headless device: {error}"),
    }
}

#[test]
fn a_headless_device_draws_into_a_render_target() {
    let Some(mut device) = headless_device() else {
        return;
    };
    let target = device
        .create_render_target(UVec2::new(2, 2), TextureFormat::Rgba8)
        .expect("a render target");
    device.begin_offscreen_frame().expect("a frame to begin");
    // A pass that only clears, and ends at once.
    let _pass = device
        .begin_pass(target.into(), Some(LinearColor::new(0.2, 0.4, 0.6, 0.8)))
        .expect("a pass that clears");
    device.end_offscreen_frame().expect("a frame to end");
    let pixels = device.read_pixels(target).expect("the target's pixels");
    assert_eq!(pixels, [51, 102, 153, 204].repeat(4));
}

#[test]
fn a_device_on_a_context_it_does_not_own_has_no_offscreen_frame() {
    let Some(mut gpu) = Gpu::new() else { return };
    assert_eq!(
        gpu.device.begin_offscreen_frame(),
        Err(GfxError::WrongSurface)
    );
    assert_eq!(
        gpu.device.end_offscreen_frame(),
        Err(GfxError::WrongSurface)
    );
}

#[test]
fn a_thread_has_one_offscreen_context_at_a_time() {
    let Some(first) = headless_device() else {
        return;
    };
    assert_eq!(
        Device::new_headless().map(drop),
        Err(GfxError::OffscreenContext(PlatformError::GlContextExists))
    );
    drop(first);
    let again = headless_device().expect("a device once the first is gone");
    drop(again);
}
