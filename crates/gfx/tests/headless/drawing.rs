//! Drawing: what lands in a target, and the draws that are refused.

use nadir_core::UVec2;
use nadir_gfx::{
    BlendMode, GfxError, IndexFormat, PipelineDesc, Sampler, TextureDesc, TextureFormat, Topology,
    UniformBlockDesc, VertexAttribute, VertexFormat, VertexLayout, VertexStep,
};

use crate::common::{
    COLOR_FS, FULL, Gpu, INDEX_FORMATS, POSITIONS, UNIT, color_pipeline, instanced_pipeline,
    linear, near, rect, shaders, srgb8, strip, textured_pipeline, textured_quad,
};

#[test]
fn a_cleared_target_reads_back_its_color() {
    let Some(mut gpu) = Gpu::new() else { return };
    let rgba = gpu.target(4, 3, TextureFormat::Rgba8);
    gpu.clear(rgba, [0.2, 0.4, 0.6, 1.0]);
    assert!(gpu.read(rgba).iter().all(|&p| p == [51, 102, 153, 255]));
    assert_eq!(gpu.read(rgba).len(), 12);

    // An sRGB target stores the encoded color.
    let srgb = gpu.target(2, 2, TextureFormat::Rgba8Srgb);
    gpu.clear(srgb, [0.5, 0.5, 0.5, 1.0]);
    let encoded = srgb8(0.5);
    assert!(
        gpu.read(srgb)
            .iter()
            .all(|&p| near(p, [encoded, encoded, encoded, 255], 1))
    );

    let r8 = gpu.target(2, 2, TextureFormat::R8);
    gpu.clear(r8, [0.4, 0.8, 0.8, 0.8]);
    assert!(gpu.read(r8).iter().all(|&p| p == [102, 0, 0, 255]));
}

#[test]
fn the_top_of_clip_space_is_the_first_row_read_back() {
    let Some(mut gpu) = Gpu::new() else { return };
    let target = gpu.target(4, 4, TextureFormat::Rgba8);
    let pipeline = color_pipeline(&mut gpu, TextureFormat::Rgba8, BlendMode::Opaque);
    let top_half = gpu.vertices(&strip(-1.0, 0.0, 1.0, 1.0));
    gpu.clear(target, [0.0, 0.0, 0.0, 1.0]);
    gpu.fill(target, pipeline, top_half, [0.8, 0.0, 0.0, 1.0]);
    let pixels = gpu.read(target);
    for (i, pixel) in pixels.iter().enumerate() {
        let expected = if i < 8 {
            [204, 0, 0, 255]
        } else {
            [0, 0, 0, 255]
        };
        assert_eq!(*pixel, expected, "pixel {i}");
    }
}

#[test]
fn a_scissor_counts_from_the_top_left_and_ends_with_its_pass() {
    let Some(mut gpu) = Gpu::new() else { return };
    let target = gpu.target(4, 4, TextureFormat::Rgba8);
    let pipeline = color_pipeline(&mut gpu, TextureFormat::Rgba8, BlendMode::Opaque);
    let quad = gpu.vertices(&FULL);
    gpu.clear(target, [0.0, 0.0, 0.0, 1.0]);
    gpu.begin();
    {
        let mut pass = gpu.device.begin_pass(target.into(), None).unwrap();
        pass.set_pipeline(pipeline).unwrap();
        pass.set_vertex_buffer(0, quad, 0).unwrap();
        pass.set_uniforms(0, &[0.2f32, 0.4, 0.6, 1.0]).unwrap();
        pass.set_scissor(Some(rect(0, 0, 2, 1)));
        pass.draw(0..4, 0..1).unwrap();
        // A scissor outside the target draws nothing, and is no error.
        pass.set_scissor(Some(rect(8, 8, 9, 9)));
        pass.draw(0..4, 0..1).unwrap();
    }
    gpu.end();
    let pixels = gpu.read(target);
    for (i, pixel) in pixels.iter().enumerate() {
        let expected = if i < 2 {
            [51, 102, 153, 255]
        } else {
            [0, 0, 0, 255]
        };
        assert_eq!(*pixel, expected, "pixel {i}");
    }
    // The next pass clears, and draws, the whole target again.
    gpu.clear(target, [0.4, 0.0, 0.0, 1.0]);
    assert!(gpu.read(target).iter().all(|&p| p == [102, 0, 0, 255]));
}

#[test]
fn each_pass_starts_with_the_whole_target_as_viewport() {
    let Some(mut gpu) = Gpu::new() else { return };
    let target = gpu.target(8, 8, TextureFormat::Rgba8);
    let pipeline = color_pipeline(&mut gpu, TextureFormat::Rgba8, BlendMode::Opaque);
    let quad = gpu.vertices(&FULL);
    gpu.clear(target, [0.0, 0.0, 0.0, 1.0]);
    gpu.begin();
    {
        let mut pass = gpu.device.begin_pass(target.into(), None).unwrap();
        pass.set_pipeline(pipeline).unwrap();
        pass.set_vertex_buffer(0, quad, 0).unwrap();
        pass.set_uniforms(0, &[0.2f32, 0.0, 0.0, 1.0]).unwrap();
        pass.set_viewport(rect(0, 0, 1, 1));
        pass.draw(0..4, 0..1).unwrap();
    }
    {
        let mut pass = gpu.device.begin_pass(target.into(), None).unwrap();
        pass.set_pipeline(pipeline).unwrap();
        pass.set_vertex_buffer(0, quad, 0).unwrap();
        pass.set_uniforms(0, &[0.4f32, 0.0, 0.0, 1.0]).unwrap();
        pass.draw(0..4, 0..1).unwrap();
    }
    gpu.end();
    assert!(gpu.read(target).iter().all(|&p| p == [102, 0, 0, 255]));
}

#[test]
fn each_sampler_filters_and_wraps_as_it_says() {
    let Some(mut gpu) = Gpu::new() else { return };
    // Black then white, sampled across 8 pixels from u = -0.5 to 1.5.
    let texture = gpu
        .device
        .create_texture(
            &TextureDesc {
                label: "black and white",
                size: UVec2::new(2, 1),
                format: TextureFormat::Rgba8,
            },
            Some(&[0, 0, 0, 255, 255, 255, 255, 255]),
        )
        .unwrap();
    let target = gpu.target(8, 1, TextureFormat::Rgba8);
    let pipeline = textured_pipeline(&mut gpu, TextureFormat::Rgba8);
    let quad = textured_quad(&mut gpu, [-0.5, 1.5], [0.0, 1.0]);
    let cases = [
        (Sampler::NearestClamp, [0, 0, 0, 0, 255, 255, 255, 255]),
        (Sampler::NearestRepeat, [255, 255, 0, 0, 255, 255, 0, 0]),
        (Sampler::LinearClamp, [0, 0, 0, 64, 191, 255, 255, 255]),
        (Sampler::LinearRepeat, [191, 191, 64, 64, 191, 191, 64, 64]),
    ];
    for (sampler, expected) in cases {
        gpu.begin();
        {
            let mut pass = gpu.device.begin_pass(target.into(), None).unwrap();
            pass.set_pipeline(pipeline).unwrap();
            pass.set_vertex_buffer(0, quad, 0).unwrap();
            pass.set_texture(0, texture, sampler).unwrap();
            pass.draw(0..4, 0..1).unwrap();
        }
        gpu.end();
        let reds: Vec<u8> = gpu.read(target).iter().map(|p| p[0]).collect();
        assert_eq!(reds, expected, "{sampler:?}");
    }
}

#[test]
fn srgb_texels_are_decoded_and_encoded_again() {
    let Some(mut gpu) = Gpu::new() else { return };
    let texels = [
        [128u8, 64, 200, 128],
        [51, 102, 153, 204],
        [1, 2, 3, 4],
        [250, 251, 252, 253],
    ];
    let texture = gpu
        .device
        .create_texture(
            &TextureDesc {
                label: "srgb",
                size: UVec2::new(4, 1),
                format: TextureFormat::Rgba8Srgb,
            },
            Some(texels.as_flattened()),
        )
        .unwrap();
    for format in [TextureFormat::Rgba8Srgb, TextureFormat::Rgba8] {
        let target = gpu.target(4, 1, format);
        let pipeline = textured_pipeline(&mut gpu, format);
        let quad = textured_quad(&mut gpu, [0.0, 1.0], [0.0, 1.0]);
        gpu.begin();
        {
            let mut pass = gpu.device.begin_pass(target.into(), None).unwrap();
            pass.set_pipeline(pipeline).unwrap();
            pass.set_vertex_buffer(0, quad, 0).unwrap();
            pass.set_texture(0, texture, Sampler::NearestClamp).unwrap();
            pass.draw(0..4, 0..1).unwrap();
        }
        gpu.end();
        let pixels = gpu.read(target);
        for (pixel, texel) in pixels.iter().zip(texels) {
            if format == TextureFormat::Rgba8Srgb {
                // Decoded, then encoded again: the same bytes.
                assert_eq!(*pixel, texel);
            } else {
                // Decoded only. Alpha is never encoded.
                let decoded = texel.map(|c| (linear(c) * 255.0).round() as u8);
                let expected = [decoded[0], decoded[1], decoded[2], texel[3]];
                assert!(near(*pixel, expected, 1), "{pixel:?} for {texel:?}");
                assert_eq!(pixel[3], texel[3]);
            }
        }
    }
}

#[test]
fn each_blend_mode_mixes_as_it_says() {
    let Some(mut gpu) = Gpu::new() else { return };
    let quad = gpu.vertices(&FULL);
    let dst = [0.2, 0.4, 0.6, 1.0];
    let unit = |value: f32| (value * 255.0).round() as u8;
    // Source colors, and what they give over `dst`, in linear light.
    let cases = [
        (
            BlendMode::Opaque,
            [0.2, 0.4, 0.6, 0.8],
            [0.2, 0.4, 0.6, 0.8],
        ),
        (
            BlendMode::Alpha,
            [0.8, 0.4, 0.2, 0.6],
            [
                0.8 * 0.6 + 0.2 * 0.4,
                0.4 * 0.6 + 0.4 * 0.4,
                0.2 * 0.6 + 0.6 * 0.4,
                1.0,
            ],
        ),
        (
            BlendMode::PremultipliedAlpha,
            [0.4, 0.2, 0.0, 0.6],
            [0.4 + 0.2 * 0.4, 0.2 + 0.4 * 0.4, 0.6 * 0.4, 1.0],
        ),
        (
            BlendMode::Additive,
            [0.4, 0.2, 0.2, 0.6],
            [0.4 * 0.6 + 0.2, 0.2 * 0.6 + 0.4, 0.2 * 0.6 + 0.6, 1.0],
        ),
        (
            BlendMode::Multiply,
            [0.6, 0.8, 0.4, 0.3],
            [0.6 * 0.2, 0.8 * 0.4, 0.4 * 0.6, 1.0],
        ),
    ];
    let target = gpu.target(2, 2, TextureFormat::Rgba8);
    for (mode, src, out) in cases {
        let pipeline = color_pipeline(&mut gpu, TextureFormat::Rgba8, mode);
        gpu.clear(target, dst);
        gpu.fill(target, pipeline, quad, src);
        let expected = out.map(unit);
        for pixel in gpu.read(target) {
            assert!(
                near(pixel, expected, 2),
                "{mode:?}: {pixel:?}, not {expected:?}"
            );
        }
    }

    // Into an sRGB target, the mix is of linear light: the target's color is
    // decoded first, and the result encoded.
    let srgb = gpu.target(2, 2, TextureFormat::Rgba8Srgb);
    let pipeline = color_pipeline(&mut gpu, TextureFormat::Rgba8Srgb, BlendMode::Alpha);
    gpu.clear(srgb, dst);
    gpu.fill(srgb, pipeline, quad, [0.8, 0.4, 0.2, 0.6]);
    let dst = [srgb8(0.2), srgb8(0.4), srgb8(0.6)].map(linear);
    let src = [0.8, 0.4, 0.2];
    let expected = [0, 1, 2].map(|i| srgb8(src[i] * 0.6 + dst[i] * 0.4));
    for pixel in gpu.read(srgb) {
        let expected = [expected[0], expected[1], expected[2], 255];
        assert!(near(pixel, expected, 2), "{pixel:?}, not {expected:?}");
    }
}

#[test]
fn an_opaque_draw_after_a_blended_one_is_not_blended() {
    let Some(mut gpu) = Gpu::new() else { return };
    let target = gpu.target(2, 2, TextureFormat::Rgba8);
    let alpha = color_pipeline(&mut gpu, TextureFormat::Rgba8, BlendMode::Alpha);
    let opaque = color_pipeline(&mut gpu, TextureFormat::Rgba8, BlendMode::Opaque);
    let quad = gpu.vertices(&FULL);
    gpu.clear(target, [0.0, 0.0, 0.0, 1.0]);
    gpu.begin();
    {
        let mut pass = gpu.device.begin_pass(target.into(), None).unwrap();
        pass.set_vertex_buffer(0, quad, 0).unwrap();
        pass.set_uniforms(0, &[0.8f32, 0.8, 0.8, 0.2]).unwrap();
        pass.set_pipeline(alpha).unwrap();
        pass.draw(0..4, 0..1).unwrap();
        pass.set_pipeline(opaque).unwrap();
        pass.draw(0..4, 0..1).unwrap();
    }
    gpu.end();
    assert!(gpu.read(target).iter().all(|&p| p == [204, 204, 204, 51]));
}

/// The instance data of four rectangles, one a pixel of a 4 by 1 target,
/// each in a color of its own.
const INSTANCES: [f32; 32] = [
    -1.0, -1.0, -0.5, 1.0, 0.2, 0.0, 0.0, 1.0, //
    -0.5, -1.0, 0.0, 1.0, 0.0, 0.4, 0.0, 1.0, //
    0.0, -1.0, 0.5, 1.0, 0.0, 0.0, 0.6, 1.0, //
    0.5, -1.0, 1.0, 1.0, 0.8, 0.8, 0.0, 1.0,
];

const INSTANCE_PIXELS: [[u8; 4]; 4] = [
    [51, 0, 0, 255],
    [0, 102, 0, 255],
    [0, 0, 153, 255],
    [204, 204, 0, 255],
];

#[test]
fn instances_read_their_own_records_from_the_first_one_drawn() {
    let Some(mut gpu) = Gpu::new() else { return };
    let target = gpu.target(4, 1, TextureFormat::Rgba8);
    let pipeline = instanced_pipeline(&mut gpu);
    let corners = gpu.vertices(&UNIT);
    let instances = gpu.vertices(&INSTANCES);
    let black = [0, 0, 0, 255];
    for (range, lit) in [(0..4, 0..4), (2..4, 2..4), (1..2, 1..2)] {
        gpu.clear(target, [0.0, 0.0, 0.0, 1.0]);
        gpu.begin();
        {
            let mut pass = gpu.device.begin_pass(target.into(), None).unwrap();
            pass.set_pipeline(pipeline).unwrap();
            pass.set_vertex_buffer(0, corners, 0).unwrap();
            pass.set_vertex_buffer(1, instances, 0).unwrap();
            pass.draw(0..4, range.clone()).unwrap();
        }
        gpu.end();
        let pixels = gpu.read(target);
        for (i, pixel) in pixels.iter().enumerate() {
            let expected = if lit.contains(&i) {
                INSTANCE_PIXELS[i]
            } else {
                black
            };
            assert_eq!(*pixel, expected, "instances {range:?}, pixel {i}");
        }
    }
}

#[test]
fn indices_plus_the_base_vertex_name_the_vertices_drawn() {
    let Some(mut gpu) = Gpu::new() else { return };
    let target = gpu.target(4, 1, TextureFormat::Rgba8);
    let pipeline_triangles = gpu
        .device
        .create_pipeline(&PipelineDesc {
            label: "triangles",
            shader: shaders(crate::common::POSITION_VS, COLOR_FS),
            vertex_layouts: &POSITIONS,
            uniform_blocks: &crate::common::COLOR_BLOCK,
            textures: &[],
            blend: BlendMode::Opaque,
            topology: Topology::Triangles,
            target_format: TextureFormat::Rgba8,
        })
        .unwrap();
    // Two quads: the left half, then the right half.
    let mut quads = strip(-1.0, -1.0, 0.0, 1.0).to_vec();
    quads.extend(strip(0.0, -1.0, 1.0, 1.0));
    let vertices = gpu.vertices(&quads);
    for format in INDEX_FORMATS {
        let indices = match format {
            IndexFormat::U16 => gpu.indices_u16(&[0, 1, 2, 2, 1, 3]),
            IndexFormat::U32 => gpu.indices_u32(&[0, 1, 2, 2, 1, 3]),
        };
        gpu.clear(target, [0.0, 0.0, 0.0, 1.0]);
        gpu.begin();
        {
            let mut pass = gpu.device.begin_pass(target.into(), None).unwrap();
            pass.set_pipeline(pipeline_triangles).unwrap();
            pass.set_vertex_buffer(0, vertices, 0).unwrap();
            pass.set_index_buffer(indices, format).unwrap();
            pass.set_uniforms(0, &[0.6f32, 0.0, 0.0, 1.0]).unwrap();
            pass.draw_indexed(0..6, 4, 0..1).unwrap();
            // One more vertex would name vertex 8, past the buffer.
            assert_eq!(
                pass.draw_indexed(0..6, 5, 0..1),
                Err(GfxError::OutOfBounds("a vertex range"))
            );
            assert!(matches!(
                pass.draw_indexed(0..7, 0, 0..1),
                Err(GfxError::OutOfBounds(_))
            ));
        }
        gpu.end();
        let reds: Vec<u8> = gpu.read(target).iter().map(|p| p[0]).collect();
        assert_eq!(reds, [0, 0, 153, 153], "{format:?}");
    }
}

/// Positions, moved and scaled by a `Camera` block, in a color from a
/// `Color` block.
const CAMERA_VS: &str = "#version 330 core
layout(std140) uniform Camera {
    vec4 offset_scale;
};
layout(location = 0) in vec2 position;
void main() {
    gl_Position = vec4(position * offset_scale.zw + offset_scale.xy, 0.0, 1.0);
}
";

#[test]
fn uniforms_reach_each_draw_through_many_draws() {
    let Some(mut gpu) = Gpu::new() else { return };
    // Enough draws to fill the uniform ring more than once.
    const DRAWS: u32 = 300;
    let blocks = [
        UniformBlockDesc {
            name: "Color",
            binding: 3,
            size: 16,
        },
        UniformBlockDesc {
            name: "Camera",
            binding: 1,
            size: 16,
        },
    ];
    let pipeline = gpu
        .device
        .create_pipeline(&PipelineDesc {
            label: "camera",
            shader: shaders(CAMERA_VS, COLOR_FS),
            vertex_layouts: &POSITIONS,
            uniform_blocks: &blocks,
            textures: &[],
            blend: BlendMode::Opaque,
            topology: Topology::TriangleStrip,
            target_format: TextureFormat::Rgba8,
        })
        .unwrap();
    let target = gpu.target(DRAWS, 1, TextureFormat::Rgba8);
    let quad = gpu.vertices(&FULL);
    let level = |i: u32| (i % 6) as f32 * 0.2;
    let color = |i: u32| [level(i), level(i / 6), level(i / 36), 1.0];
    gpu.clear(target, [0.0, 0.0, 0.0, 0.0]);
    gpu.begin();
    {
        let mut pass = gpu.device.begin_pass(target.into(), None).unwrap();
        pass.set_pipeline(pipeline).unwrap();
        pass.set_vertex_buffer(0, quad, 0).unwrap();
        // Set once, and needed by every draw after the ring is full.
        pass.set_uniforms(1, &[0.0f32, 0.0, 1.0, 1.0]).unwrap();
        for i in 0..DRAWS {
            pass.set_viewport(rect(i as i32, 0, i as i32 + 1, 1));
            pass.set_uniforms(3, &color(i)).unwrap();
            pass.draw(0..4, 0..1).unwrap();
        }
    }
    gpu.end();
    let fifths = |value: f32| (value * 5.0).round() as u8 * 51;
    for (i, pixel) in gpu.read(target).iter().enumerate() {
        let expected = color(i as u32).map(fifths);
        assert_eq!(*pixel, expected, "draw {i}");
    }
}

#[test]
fn draws_that_would_read_past_a_buffer_are_refused() {
    let Some(mut gpu) = Gpu::new() else { return };
    let target = gpu.target(4, 1, TextureFormat::Rgba8);
    let pipeline = instanced_pipeline(&mut gpu);
    let colored = color_pipeline(&mut gpu, TextureFormat::Rgba8, BlendMode::Opaque);
    let textured = textured_pipeline(&mut gpu, TextureFormat::Rgba8);
    let corners = gpu.vertices(&UNIT);
    let instances = gpu.vertices(&INSTANCES);
    gpu.clear(target, [0.0, 0.0, 0.0, 1.0]);
    gpu.begin();
    {
        let mut pass = gpu.device.begin_pass(target.into(), None).unwrap();
        assert_eq!(pass.draw(0..4, 0..1), Err(GfxError::NoPipeline));
        // Slots may be set before the pipeline.
        pass.set_vertex_buffer(0, corners, 0).unwrap();
        pass.set_pipeline(pipeline).unwrap();
        assert_eq!(
            pass.draw(0..4, 0..1),
            Err(GfxError::Unbound {
                what: "vertex buffer",
                slot: 1
            })
        );
        pass.set_vertex_buffer(1, instances, 0).unwrap();
        assert!(matches!(
            pass.draw(0..5, 0..1),
            Err(GfxError::OutOfBounds(_))
        ));
        assert!(matches!(
            pass.draw(0..4, 0..5),
            Err(GfxError::OutOfBounds(_))
        ));
        assert!(matches!(
            pass.draw(0..4, 4..5),
            Err(GfxError::OutOfBounds(_))
        ));
        // A slot that starts later has fewer records.
        pass.set_vertex_buffer(1, instances, 32).unwrap();
        assert!(matches!(
            pass.draw(0..4, 0..4),
            Err(GfxError::OutOfBounds(_))
        ));
        assert!(matches!(
            pass.draw_indexed(0..3, 0, 0..1),
            Err(GfxError::Unbound { .. })
        ));
        pass.set_vertex_buffer(1, instances, 0).unwrap();
        pass.set_pipeline(colored).unwrap();
        assert_eq!(
            pass.draw(0..4, 0..1),
            Err(GfxError::Unbound {
                what: "uniform block",
                slot: 0
            })
        );
        pass.set_pipeline(textured).unwrap();
        assert!(matches!(
            pass.draw(0..1, 0..1),
            Err(GfxError::Unbound { .. })
        ));
        // Empty ranges draw nothing, and are no error.
        assert_eq!(pass.draw(2..2, 0..1), Ok(()));
        assert_eq!(pass.draw(0..4, 1..1), Ok(()));
    }
    gpu.end();
    // Nothing was drawn.
    assert!(gpu.read(target).iter().all(|&p| p == [0, 0, 0, 255]));
}

#[test]
fn a_record_ends_where_its_furthest_attribute_ends() {
    let Some(mut gpu) = Gpu::new() else { return };
    let target = gpu.target(1, 1, TextureFormat::Rgba8);
    // Listed out of order: the record's 16 bytes end with the first.
    let attributes = [
        VertexAttribute {
            location: 1,
            format: VertexFormat::Float32x2,
            offset: 8,
        },
        VertexAttribute {
            location: 0,
            format: VertexFormat::Float32x2,
            offset: 0,
        },
    ];
    let layouts = [VertexLayout {
        stride: 16,
        step: VertexStep::Vertex,
        attributes: &attributes,
    }];
    let pipeline = gpu
        .device
        .create_pipeline(&PipelineDesc {
            label: "unordered",
            shader: shaders(crate::common::TEXTURED_VS, crate::common::TEXTURED_FS),
            vertex_layouts: &layouts,
            uniform_blocks: &[],
            textures: &["tex"],
            blend: BlendMode::Opaque,
            topology: Topology::TriangleStrip,
            target_format: TextureFormat::Rgba8,
        })
        .unwrap();
    let texture = gpu
        .device
        .create_texture(
            &TextureDesc {
                label: "texel",
                size: UVec2::new(1, 1),
                format: TextureFormat::Rgba8,
            },
            Some(&[51, 102, 153, 255]),
        )
        .unwrap();
    // 40 bytes: room for vertices 0 and 1, and 8 bytes of a third, which its
    // attribute at 8 would read past.
    let vertices = gpu.vertices(&[0.0; 10]);
    gpu.begin();
    {
        let mut pass = gpu.device.begin_pass(target.into(), None).unwrap();
        pass.set_pipeline(pipeline).unwrap();
        pass.set_vertex_buffer(0, vertices, 0).unwrap();
        pass.set_texture(0, texture, Sampler::NearestClamp).unwrap();
        assert_eq!(pass.draw(0..2, 0..1), Ok(()));
        assert!(matches!(
            pass.draw(0..3, 0..1),
            Err(GfxError::OutOfBounds(_))
        ));
    }
    gpu.end();
}

#[test]
fn a_pass_neither_samples_its_own_target_nor_takes_another_format() {
    let Some(mut gpu) = Gpu::new() else { return };
    let target = gpu.target(2, 2, TextureFormat::Rgba8);
    let srgb_pipeline = color_pipeline(&mut gpu, TextureFormat::Rgba8Srgb, BlendMode::Opaque);
    gpu.begin();
    {
        let mut pass = gpu.device.begin_pass(target.into(), None).unwrap();
        assert_eq!(
            pass.set_texture(0, target, Sampler::NearestClamp),
            Err(GfxError::FeedbackLoop)
        );
        assert_eq!(
            pass.set_pipeline(srgb_pipeline),
            Err(GfxError::FormatMismatch {
                pipeline: TextureFormat::Rgba8Srgb,
                target: TextureFormat::Rgba8
            })
        );
    }
    gpu.end();
}

#[test]
fn a_render_target_samples_as_it_was_drawn() {
    let Some(mut gpu) = Gpu::new() else { return };
    let first = gpu.target(4, 4, TextureFormat::Rgba8);
    let second = gpu.target(4, 4, TextureFormat::Rgba8);
    let colored = color_pipeline(&mut gpu, TextureFormat::Rgba8, BlendMode::Opaque);
    let top_half = gpu.vertices(&strip(-1.0, 0.0, 1.0, 1.0));
    gpu.clear(first, [0.0, 0.0, 0.0, 1.0]);
    gpu.fill(first, colored, top_half, [0.8, 0.0, 0.0, 1.0]);
    // Stored from its bottom row, a render target maps clip space's -1..1 to
    // v = 0..1.
    let pipeline = textured_pipeline(&mut gpu, TextureFormat::Rgba8);
    let quad = textured_quad(&mut gpu, [0.0, 1.0], [0.0, 1.0]);
    gpu.begin();
    {
        let mut pass = gpu.device.begin_pass(second.into(), None).unwrap();
        pass.set_pipeline(pipeline).unwrap();
        pass.set_vertex_buffer(0, quad, 0).unwrap();
        pass.set_texture(0, first, Sampler::NearestClamp).unwrap();
        pass.draw(0..4, 0..1).unwrap();
    }
    gpu.end();
    assert_eq!(gpu.read(second), gpu.read(first));
}

#[test]
fn a_new_resource_under_a_reused_gl_name_is_drawn_with_its_own_contents() {
    let Some(mut gpu) = Gpu::new() else { return };
    let target = gpu.target(1, 1, TextureFormat::Rgba8);
    let pipeline = textured_pipeline(&mut gpu, TextureFormat::Rgba8);
    let quad = textured_quad(&mut gpu, [0.0, 1.0], [0.0, 1.0]);
    let desc = TextureDesc {
        label: "one texel",
        size: UVec2::new(1, 1),
        format: TextureFormat::Rgba8,
    };
    let draw_with = |gpu: &mut Gpu, texture| {
        gpu.begin();
        {
            let mut pass = gpu.device.begin_pass(target.into(), None).unwrap();
            pass.set_pipeline(pipeline).unwrap();
            pass.set_vertex_buffer(0, quad, 0).unwrap();
            pass.set_texture(0, texture, Sampler::NearestClamp).unwrap();
            pass.draw(0..4, 0..1).unwrap();
        }
        gpu.end();
        gpu.read(target)[0]
    };
    let red = gpu
        .device
        .create_texture(&desc, Some(&[204, 0, 0, 255]))
        .unwrap();
    assert_eq!(draw_with(&mut gpu, red), [204, 0, 0, 255]);
    // GL hands the deleted name out again at once.
    gpu.device.destroy(red);
    let blue = gpu
        .device
        .create_texture(&desc, Some(&[0, 0, 153, 255]))
        .unwrap();
    assert_eq!(draw_with(&mut gpu, blue), [0, 0, 153, 255]);

    // The same for a vertex buffer that a vertex array points at.
    let colored = color_pipeline(&mut gpu, TextureFormat::Rgba8, BlendMode::Opaque);
    let wide = gpu.target(2, 1, TextureFormat::Rgba8);
    let left = gpu.vertices(&strip(-1.0, -1.0, 0.0, 1.0));
    gpu.clear(wide, [0.0, 0.0, 0.0, 1.0]);
    gpu.fill(wide, colored, left, [0.4, 0.0, 0.0, 1.0]);
    gpu.device.destroy(left);
    let right = gpu.vertices(&strip(0.0, -1.0, 1.0, 1.0));
    gpu.clear(wide, [0.0, 0.0, 0.0, 1.0]);
    gpu.fill(wide, colored, right, [0.4, 0.0, 0.0, 1.0]);
    let reds: Vec<u8> = gpu.read(wide).iter().map(|p| p[0]).collect();
    assert_eq!(reds, [0, 102]);
}
