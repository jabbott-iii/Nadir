//! Building pipelines: what a shader that does not compile or link says,
//! and descriptions that do not match their shaders.

use nadir_gfx::{
    BlendMode, GfxError, PipelineDesc, ShaderStage, TextureFormat, Topology, UniformBlockDesc,
};

use crate::common::{
    COLOR_BLOCK, COLOR_FS, FULL, Gpu, POSITION_VS, POSITIONS, color_pipeline, shaders,
};

fn desc<'a>(vs: &'a str, fs: &'a str, blocks: &'a [UniformBlockDesc<'a>]) -> PipelineDesc<'a> {
    PipelineDesc {
        label: "broken",
        shader: shaders(vs, fs),
        vertex_layouts: &POSITIONS,
        uniform_blocks: blocks,
        textures: &[],
        blend: BlendMode::Opaque,
        topology: Topology::TriangleStrip,
        target_format: TextureFormat::Rgba8,
    }
}

const BAD_VS: &str = "#version 330 core
void main() {
    gl_Position = undefined_thing;
}
";

const BAD_FS: &str = "#version 330 core
out vec4 out_color;
void main() {
    out_color = undefined_thing;
}
";

/// Declares a function that nothing defines, which only the linker finds.
const UNLINKED_VS: &str = "#version 330 core
vec4 helper_never_defined();
void main() {
    gl_Position = helper_never_defined();
}
";

/// The stage and log of a pipeline that fails to build.
fn failure(gpu: &mut Gpu, desc: &PipelineDesc<'_>) -> (ShaderStage, String, String) {
    match gpu.device.create_pipeline(desc) {
        Err(error @ GfxError::Shader { .. }) => {
            let message = error.to_string();
            let GfxError::Shader { label, stage, log } = error else {
                unreachable!()
            };
            assert_eq!(label, "broken");
            assert!(
                message.contains("broken") && message.contains(&log),
                "{message}"
            );
            (stage, log, message)
        }
        other => panic!("not a shader error: {other:?}"),
    }
}

#[test]
fn a_shader_that_does_not_compile_gives_its_stage_and_the_drivers_log() {
    let Some(mut gpu) = Gpu::new() else { return };
    let (stage, log, message) = failure(&mut gpu, &desc(BAD_VS, COLOR_FS, &COLOR_BLOCK));
    assert_eq!(stage, ShaderStage::Vertex);
    assert!(log.contains("undefined_thing"), "{log}");
    assert!(message.contains("vertex shader"), "{message}");

    let (stage, log, message) = failure(&mut gpu, &desc(POSITION_VS, BAD_FS, &[]));
    assert_eq!(stage, ShaderStage::Fragment);
    assert!(log.contains("undefined_thing"), "{log}");
    assert!(message.contains("fragment shader"), "{message}");

    // With both broken, the first stage is the one reported.
    let (stage, _, _) = failure(&mut gpu, &desc(BAD_VS, BAD_FS, &[]));
    assert_eq!(stage, ShaderStage::Vertex);
}

#[test]
fn shaders_that_do_not_link_give_the_linkers_log() {
    let Some(mut gpu) = Gpu::new() else { return };
    let (stage, log, message) = failure(&mut gpu, &desc(UNLINKED_VS, COLOR_FS, &COLOR_BLOCK));
    assert_eq!(stage, ShaderStage::Link);
    assert!(log.contains("helper_never_defined"), "{log}");
    assert!(message.contains("did not link"), "{message}");
}

fn interface_error(gpu: &mut Gpu, desc: &PipelineDesc<'_>) -> String {
    match gpu.device.create_pipeline(desc) {
        Err(GfxError::Interface { label, reason }) => {
            assert_eq!(label, "broken");
            reason
        }
        other => panic!("not an interface error: {other:?}"),
    }
}

#[test]
fn a_description_must_match_its_shaders() {
    let Some(mut gpu) = Gpu::new() else { return };
    let reason = interface_error(&mut gpu, &desc("void main() {}", COLOR_FS, &COLOR_BLOCK));
    assert!(reason.contains("#version 330 core"), "{reason}");

    // The block is a `vec4`: 16 bytes.
    let wrong_size = [UniformBlockDesc {
        name: "Color",
        binding: 0,
        size: 32,
    }];
    let reason = interface_error(&mut gpu, &desc(POSITION_VS, COLOR_FS, &wrong_size));
    assert!(reason.contains("16 bytes in the shaders"), "{reason}");

    // A block the description leaves out would read whatever binding 0
    // holds.
    let reason = interface_error(&mut gpu, &desc(POSITION_VS, COLOR_FS, &[]));
    assert!(reason.contains("uniform blocks"), "{reason}");

    let bad_binding = [UniformBlockDesc {
        name: "Color",
        binding: 8,
        size: 16,
    }];
    interface_error(&mut gpu, &desc(POSITION_VS, COLOR_FS, &bad_binding));
}

/// Two blocks of an array, which `B` and `B[0]` both name the first of.
const BLOCK_ARRAY_FS: &str = "#version 330 core
layout(std140) uniform B {
    vec4 v;
} b[2];
out vec4 out_color;
void main() {
    out_color = b[0].v + b[1].v;
}
";

#[test]
fn two_names_for_one_uniform_block_are_refused() {
    let Some(mut gpu) = Gpu::new() else { return };
    let block = |name, binding| UniformBlockDesc {
        name,
        binding,
        size: 16,
    };
    // `b[1]` would keep binding 0, which nothing sets.
    let aliased = [block("B", 0), block("B[0]", 1)];
    let reason = interface_error(&mut gpu, &desc(POSITION_VS, BLOCK_ARRAY_FS, &aliased));
    assert!(reason.contains("name the same uniform block"), "{reason}");
    let each = [block("B[0]", 0), block("B[1]", 1)];
    assert!(
        gpu.device
            .create_pipeline(&desc(POSITION_VS, BLOCK_ARRAY_FS, &each))
            .is_ok()
    );
}

#[test]
fn a_shader_may_not_enable_extensions() {
    let Some(mut gpu) = Gpu::new() else { return };
    let fs = "#version 330 core
#extension GL_ARB_shader_storage_buffer_object : require
out vec4 out_color;
void main() {
    out_color = vec4(1.0);
}
";
    let reason = interface_error(&mut gpu, &desc(POSITION_VS, fs, &[]));
    assert!(reason.contains("#extension"), "{reason}");
}

#[test]
fn names_the_shaders_do_not_use_are_only_warned_about() {
    let Some(mut gpu) = Gpu::new() else { return };
    let blocks = [
        COLOR_BLOCK[0],
        UniformBlockDesc {
            name: "Unused",
            binding: 1,
            size: 16,
        },
    ];
    let mut with_extras = desc(POSITION_VS, COLOR_FS, &blocks);
    with_extras.textures = &["no_such_sampler"];
    assert!(gpu.device.create_pipeline(&with_extras).is_ok());
}

#[test]
fn uniform_data_is_limited_to_256_bytes() {
    let Some(mut gpu) = Gpu::new() else { return };
    let target = gpu.target(1, 1, TextureFormat::Rgba8);
    let pipeline = color_pipeline(&mut gpu, TextureFormat::Rgba8, BlendMode::Opaque);
    let quad = gpu.vertices(&FULL);
    gpu.begin();
    {
        let mut pass = gpu.device.begin_pass(target.into(), None).unwrap();
        pass.set_pipeline(pipeline).unwrap();
        pass.set_vertex_buffer(0, quad, 0).unwrap();
        assert!(matches!(
            pass.set_uniforms(0, &[0u8; 257]),
            Err(GfxError::InvalidArgument(_))
        ));
        assert!(matches!(
            pass.set_uniforms(8, &[0.0f32; 4]),
            Err(GfxError::InvalidArgument(_))
        ));
        // A failed setter leaves the binding unset, even after good data.
        pass.set_uniforms(0, &[0.0f32; 4]).unwrap();
        assert!(pass.set_uniforms(0, &[0u8; 257]).is_err());
        assert_eq!(
            pass.draw(0..4, 0..1),
            Err(GfxError::Unbound {
                what: "uniform block",
                slot: 0
            })
        );
        // Too few bytes for the block is refused at the draw.
        pass.set_uniforms(0, &[0.0f32; 2]).unwrap();
        assert_eq!(
            pass.draw(0..4, 0..1),
            Err(GfxError::DataSize {
                expected: 16,
                found: 8
            })
        );
    }
    gpu.end();
}
