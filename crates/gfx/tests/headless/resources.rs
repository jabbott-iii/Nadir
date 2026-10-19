//! Textures, buffers and render targets, and what a destroyed one gives.

use nadir_core::UVec2;
use nadir_gfx::{
    BlendMode, BufferDesc, BufferKind, BufferUsage, GfxError, IndexFormat, ResourceKind, Sampler,
    TextureDesc, TextureFormat, TextureId,
};
use nadir_platform::GpuSurface;

use crate::common::{FULL, Gpu, color_pipeline, rect, textured_pipeline, textured_quad};

fn texture_desc(width: u32, height: u32, format: TextureFormat) -> TextureDesc<'static> {
    TextureDesc {
        label: "test",
        size: UVec2::new(width, height),
        format,
    }
}

#[test]
fn a_destroyed_resource_is_a_stale_handle_everywhere() {
    let Some(mut gpu) = Gpu::new() else { return };
    let stale = |kind| Err(GfxError::StaleHandle(kind));

    let texture = gpu
        .device
        .create_texture(&texture_desc(2, 2, TextureFormat::Rgba8), None)
        .unwrap();
    let buffer = gpu.vertices(&FULL);
    let target = gpu.target(2, 2, TextureFormat::Rgba8);
    let canvas = gpu.target(2, 2, TextureFormat::Rgba8);
    let pipeline = color_pipeline(&mut gpu, TextureFormat::Rgba8, BlendMode::Opaque);
    let live = color_pipeline(&mut gpu, TextureFormat::Rgba8, BlendMode::Opaque);
    gpu.device.destroy(texture);
    gpu.device.destroy(buffer);
    gpu.device.destroy(target);
    gpu.device.destroy(pipeline);
    // A second destroy does nothing.
    gpu.device.destroy(texture);

    assert_eq!(
        gpu.device
            .update_texture(texture, rect(0, 0, 1, 1), &[0; 4]),
        stale(ResourceKind::Texture)
    );
    assert_eq!(
        gpu.device.write_buffer(buffer, 0, &[0.0f32]),
        stale(ResourceKind::Buffer)
    );
    assert_eq!(
        gpu.device.read_pixels(target).map(drop),
        stale(ResourceKind::RenderTarget)
    );
    gpu.begin();
    assert_eq!(
        gpu.device.begin_pass(target.into(), None).map(drop),
        stale(ResourceKind::RenderTarget)
    );
    {
        let mut pass = gpu.device.begin_pass(canvas.into(), None).unwrap();
        assert_eq!(
            pass.set_texture(0, texture, Sampler::NearestClamp),
            stale(ResourceKind::Texture)
        );
        assert_eq!(
            pass.set_texture(0, target, Sampler::NearestClamp),
            stale(ResourceKind::RenderTarget)
        );
        assert_eq!(
            pass.set_vertex_buffer(0, buffer, 0),
            stale(ResourceKind::Buffer)
        );
        assert_eq!(
            pass.set_index_buffer(buffer, IndexFormat::U16),
            stale(ResourceKind::Buffer)
        );
        pass.set_pipeline(live).unwrap();
        assert_eq!(pass.set_pipeline(pipeline), stale(ResourceKind::Pipeline));
        // The failed setting is unset, so the live pipeline set before it is
        // not drawn with either.
        assert_eq!(pass.draw(0..4, 0..1), Err(GfxError::NoPipeline));
    }
    gpu.end();

    // A new resource is not the old one under its old id.
    let again = gpu
        .device
        .create_texture(&texture_desc(2, 2, TextureFormat::Rgba8), None)
        .unwrap();
    assert_ne!(again, texture);
    assert_eq!(
        gpu.device
            .update_texture(texture, rect(0, 0, 1, 1), &[0; 4]),
        stale(ResourceKind::Texture)
    );
    assert_eq!(
        gpu.device.update_texture(again, rect(0, 0, 1, 1), &[0; 4]),
        Ok(())
    );
}

/// Draws a texture over the whole of a target of its size, and reads it.
fn sampled(gpu: &mut Gpu, texture: TextureId, size: UVec2, format: TextureFormat) -> Vec<[u8; 4]> {
    let target = gpu.target(size.x, size.y, format);
    let pipeline = textured_pipeline(gpu, format);
    // The first row of a texture is v = 0, and the top of clip space is
    // the first row read back: v runs from 1 at the bottom to 0 at the top.
    let quad = textured_quad(gpu, [0.0, 1.0], [1.0, 0.0]);
    gpu.begin();
    {
        let mut pass = gpu.device.begin_pass(target.into(), None).expect("a pass");
        pass.set_pipeline(pipeline).expect("the pipeline");
        pass.set_vertex_buffer(0, quad, 0).expect("the quad");
        pass.set_texture(0, texture, Sampler::NearestClamp)
            .expect("the texture");
        pass.draw(0..4, 0..1).expect("a draw");
    }
    gpu.end();
    gpu.read(target)
}

#[test]
fn texture_rows_come_back_from_the_first() {
    let Some(mut gpu) = Gpu::new() else { return };
    let texels: Vec<u8> = (0..2 * 3)
        .flat_map(|i| [i * 40, 255 - i * 40, 51, 255])
        .collect();
    let size = UVec2::new(2, 3);
    let texture = gpu
        .device
        .create_texture(&texture_desc(2, 3, TextureFormat::Rgba8), Some(&texels))
        .unwrap();
    let pixels = sampled(&mut gpu, texture, size, TextureFormat::Rgba8);
    let expected = texels.as_chunks::<4>().0.to_vec();
    assert_eq!(pixels, expected);
}

#[test]
fn an_update_changes_exactly_its_region() {
    let Some(mut gpu) = Gpu::new() else { return };
    let size = UVec2::new(4, 3);
    let texture = gpu
        .device
        .create_texture(
            &texture_desc(4, 3, TextureFormat::Rgba8),
            Some(&[0; 4 * 3 * 4]),
        )
        .unwrap();
    // Columns 1 and 2 of row 1.
    let white = [255u8; 2 * 4];
    gpu.device
        .update_texture(texture, rect(1, 1, 3, 2), &white)
        .unwrap();
    assert_eq!(
        gpu.device
            .update_texture(texture, rect(1, 1, 3, 2), &white[..4]),
        Err(GfxError::DataSize {
            expected: 8,
            found: 4
        })
    );
    assert_eq!(
        gpu.device.update_texture(texture, rect(3, 0, 5, 1), &white),
        Err(GfxError::OutOfBounds("a texture region"))
    );
    assert_eq!(
        gpu.device.update_texture(texture, rect(2, 2, 2, 2), &[]),
        Ok(())
    );
    let pixels = sampled(&mut gpu, texture, size, TextureFormat::Rgba8);
    for (i, pixel) in pixels.iter().enumerate() {
        let (x, y) = (i % 4, i / 4);
        let lit = y == 1 && (1..3).contains(&x);
        assert_eq!(*pixel, if lit { [255; 4] } else { [0; 4] }, "at {x}, {y}");
    }
}

#[test]
fn an_r8_texture_of_odd_width_uploads_exactly() {
    let Some(mut gpu) = Gpu::new() else { return };
    // Rows of 3 bytes, which an unpack alignment of 4 would skew.
    let texels = [10u8, 20, 30, 40, 50, 60];
    let texture = gpu
        .device
        .create_texture(&texture_desc(3, 2, TextureFormat::R8), Some(&texels))
        .unwrap();
    let pixels = sampled(&mut gpu, texture, UVec2::new(3, 2), TextureFormat::Rgba8);
    let expected: Vec<[u8; 4]> = texels.iter().map(|&r| [r, 0, 0, 255]).collect();
    assert_eq!(pixels, expected);
}

#[test]
fn texture_sizes_and_data_are_checked() {
    let Some(mut gpu) = Gpu::new() else { return };
    let max = gpu.device.caps().max_texture_size;
    assert_eq!(
        gpu.device
            .create_texture(&texture_desc(max + 1, 1, TextureFormat::Rgba8), None)
            .map(drop),
        Err(GfxError::TooLarge {
            size: UVec2::new(max + 1, 1),
            max
        })
    );
    assert!(matches!(
        gpu.device
            .create_texture(&texture_desc(0, 1, TextureFormat::Rgba8), None),
        Err(GfxError::TooLarge { .. })
    ));
    assert_eq!(
        gpu.device
            .create_texture(&texture_desc(2, 2, TextureFormat::Rgba8), Some(&[0; 15]))
            .map(drop),
        Err(GfxError::DataSize {
            expected: 16,
            found: 15
        })
    );
    assert!(matches!(
        gpu.device
            .create_render_target(UVec2::new(max + 1, 1), TextureFormat::Rgba8),
        Err(GfxError::TooLarge { .. })
    ));
}

#[test]
fn buffer_writes_must_fit() {
    let Some(mut gpu) = Gpu::new() else { return };
    let buffer = gpu
        .device
        .create_buffer(&BufferDesc {
            label: "test",
            kind: BufferKind::Vertex,
            usage: BufferUsage::Dynamic,
            size: 16,
        })
        .unwrap();
    assert_eq!(gpu.device.write_buffer(buffer, 2, &[1.0f32, 2.0]), Ok(()));
    assert_eq!(
        gpu.device.write_buffer::<f32>(buffer, 9, &[]),
        Err(GfxError::OutOfBounds("a buffer write"))
    );
    assert_eq!(
        gpu.device.write_buffer(buffer, 3, &[1.0f32, 2.0]),
        Err(GfxError::OutOfBounds("a buffer write"))
    );
    assert_eq!(
        gpu.device.write_buffer(buffer, usize::MAX, &[1.0f32]),
        Err(GfxError::OutOfBounds("a buffer write"))
    );
    assert!(matches!(
        gpu.device.create_buffer(&BufferDesc {
            label: "empty",
            kind: BufferKind::Vertex,
            usage: BufferUsage::Static,
            size: 0,
        }),
        Err(GfxError::InvalidArgument(_))
    ));
}

#[test]
fn running_out_of_memory_loses_the_device() {
    let Some(mut gpu) = Gpu::new() else { return };
    // No memory for the device's copy of an index buffer is an error, and
    // loses nothing.
    let huge_indices = BufferDesc {
        label: "huge indices",
        kind: BufferKind::Index,
        usage: BufferUsage::Static,
        size: 1 << 40,
    };
    assert_eq!(
        gpu.device.create_buffer(&huge_indices).map(drop),
        Err(GfxError::OutOfMemory(
            "the device's copy of an index buffer"
        ))
    );
    assert!(
        gpu.device
            .create_render_target(UVec2::new(1, 1), TextureFormat::Rgba8)
            .is_ok()
    );
    let huge = BufferDesc {
        label: "huge",
        kind: BufferKind::Vertex,
        usage: BufferUsage::Static,
        size: 1 << 40,
    };
    assert_eq!(
        gpu.device.create_buffer(&huge).map(drop),
        Err(GfxError::OutOfMemory("a buffer"))
    );
    assert_eq!(
        gpu.device
            .create_render_target(UVec2::new(1, 1), TextureFormat::Rgba8)
            .map(drop),
        Err(GfxError::ContextLost)
    );
    let surface = GpuSurface::Gl(&gpu.gl);
    assert_eq!(gpu.device.begin_frame(surface), Err(GfxError::ContextLost));
}
