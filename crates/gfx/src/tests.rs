//! Tests of the device that reach into its internals, or that need a
//! context which is not one of `nadir-platform`'s.

use core::ffi::{CStr, c_void};
use core::ptr;

use nadir_core::UVec2;
use nadir_platform::{GlContext, GpuSurface, PlatformError};

use crate::{Device, GfxError};

/// A context with no functions, like the stand-in of a system that has no
/// OpenGL yet.
struct NullGl;

// SAFETY: `get_proc_address` never returns a pointer, so the trait promises
// nothing else.
unsafe impl GlContext for NullGl {
    fn get_proc_address(&self, _name: &CStr) -> *const c_void {
        ptr::null()
    }

    fn swap_buffers(&self) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported)
    }

    fn set_swap_interval(&self, _interval: i32) -> bool {
        false
    }

    fn drawable_size(&self) -> UVec2 {
        UVec2::ZERO
    }
}

#[test]
fn a_context_without_functions_names_the_first_one_it_lacks() {
    assert_eq!(
        Device::new(GpuSurface::Gl(&NullGl)).err(),
        Some(GfxError::MissingFunction { name: "glGetError" })
    );
}

#[cfg(feature = "headless-test")]
mod gl {
    use core::ffi::{CStr, c_void};
    use std::env;

    use nadir_core::log;
    use nadir_core::{LinearColor, UVec2};
    use nadir_platform::{GlContext, GpuSurface, HeadlessGl, PlatformError, create_headless_gl};

    use crate::{
        BlendMode, BufferDesc, BufferKind, BufferUsage, Device, GfxError, PipelineDesc, Sampler,
        ShaderSource, TextureDesc, TextureFormat, Topology, VertexAttribute, VertexFormat,
        VertexLayout, VertexStep,
    };

    /// An offscreen context, or `None` where there is none yet and no
    /// renderer is asked for.
    fn headless() -> Option<HeadlessGl> {
        let expected = env::var("NADIR_EXPECT_GL_RENDERER").unwrap_or_default();
        match create_headless_gl() {
            Ok(gl) => Some(gl),
            Err(PlatformError::Unsupported) if expected.is_empty() => None,
            Err(error) => panic!("no offscreen OpenGL context: {error}"),
        }
    }

    /// A real context's functions, without one of them, and with or
    /// without its id.
    struct Wrapped<'a> {
        inner: &'a HeadlessGl,
        withheld: Option<&'static str>,
        with_id: bool,
    }

    // SAFETY: every pointer it gives is one that `inner` gives for the same
    // name, so `inner`'s promises hold for it, and the id is `inner`'s.
    unsafe impl GlContext for Wrapped<'_> {
        fn get_proc_address(&self, name: &CStr) -> *const c_void {
            if self
                .withheld
                .is_some_and(|withheld| name.to_bytes() == withheld.as_bytes())
            {
                return core::ptr::null();
            }
            self.inner.get_proc_address(name)
        }

        fn swap_buffers(&self) -> Result<(), PlatformError> {
            self.inner.swap_buffers()
        }

        fn set_swap_interval(&self, interval: i32) -> bool {
            self.inner.set_swap_interval(interval)
        }

        fn drawable_size(&self) -> UVec2 {
            self.inner.drawable_size()
        }

        fn context_id(&self) -> Option<nadir_platform::GlContextId> {
            if self.with_id {
                self.inner.context_id()
            } else {
                None
            }
        }
    }

    #[test]
    fn every_function_is_looked_up_before_any_is_called() {
        let Some(gl) = headless() else { return };
        for name in ["glVertexAttribDivisor", "glDrawElementsInstancedBaseVertex"] {
            let context = Wrapped {
                inner: &gl,
                withheld: Some(name),
                with_id: true,
            };
            assert_eq!(
                Device::new(GpuSurface::Gl(&context)).err(),
                Some(GfxError::MissingFunction { name })
            );
        }
    }

    #[test]
    fn a_context_without_an_id_is_refused() {
        let Some(gl) = headless() else { return };
        let anonymous = Wrapped {
            inner: &gl,
            withheld: None,
            with_id: false,
        };
        assert_eq!(
            Device::new(GpuSurface::Gl(&anonymous)).err(),
            Some(GfxError::NoContext)
        );
        // Nor can it stand in for the device's own context.
        let mut device = Device::new(GpuSurface::Gl(&gl)).unwrap();
        assert_eq!(
            device.begin_frame(GpuSurface::Gl(&anonymous)),
            Err(GfxError::WrongSurface)
        );
    }

    #[test]
    fn in_debug_builds_every_call_is_checked_and_otherwise_each_frame() {
        let Some(gl) = headless() else { return };
        let mut device = Device::new(GpuSurface::Gl(&gl)).unwrap();
        let status = device.gl().status();
        assert_eq!(status.every_call(), cfg!(debug_assertions));
        status.set_every_call(true);
        assert_eq!(
            device.gl().enable(0),
            Err(GfxError::Gl {
                call: "glEnable",
                code: nadir_sys::gl::GL_INVALID_ENUM
            })
        );
        // The error was taken: the next call is clean.
        assert_eq!(
            device.gl().enable(nadir_sys::gl::GL_FRAMEBUFFER_SRGB),
            Ok(())
        );

        device.gl().status().set_every_call(false);
        device.begin_frame(GpuSurface::Gl(&gl)).unwrap();
        assert_eq!(device.gl().enable(0), Ok(()));
        // The frame's end sees it, names the last call, and logs it.
        device.end_frame(GpuSurface::Gl(&gl)).unwrap();
        let logged = log::tail(log::RING_CAPACITY).into_iter().any(|record| {
            record.contains("OpenGL reported GL_INVALID_ENUM in this frame, up to glEnable")
        });
        assert!(logged);
    }

    const VS: &str = "#version 330 core
layout(location = 0) in vec2 position;
out vec2 v_uv;
void main() {
    v_uv = position * 0.5 + 0.5;
    gl_Position = vec4(position, 0.0, 1.0);
}
";

    const FS: &str = "#version 330 core
uniform sampler2D tex;
in vec2 v_uv;
out vec4 out_color;
void main() {
    out_color = texture(tex, v_uv);
}
";

    #[test]
    fn state_that_is_set_already_is_not_set_again_until_the_next_frame() {
        let Some(gl) = headless() else { return };
        let mut device = Device::new(GpuSurface::Gl(&gl)).unwrap();
        let attributes = [VertexAttribute {
            location: 0,
            format: VertexFormat::Float32x2,
            offset: 0,
        }];
        let layouts = [VertexLayout {
            stride: 8,
            step: VertexStep::Vertex,
            attributes: &attributes,
        }];
        let pipeline = device
            .create_pipeline(&PipelineDesc {
                label: "cached",
                shader: ShaderSource {
                    glsl_vs: VS,
                    glsl_fs: FS,
                    msl: "",
                },
                vertex_layouts: &layouts,
                uniform_blocks: &[],
                textures: &["tex"],
                blend: BlendMode::Alpha,
                topology: Topology::TriangleStrip,
                target_format: TextureFormat::Rgba8,
            })
            .unwrap();
        let quad = device
            .create_buffer(&BufferDesc {
                label: "quad",
                kind: BufferKind::Vertex,
                usage: BufferUsage::Static,
                size: 32,
            })
            .unwrap();
        device
            .write_buffer(quad, 0, &[-1.0f32, -1.0, 1.0, -1.0, -1.0, 1.0, 1.0, 1.0])
            .unwrap();
        let texture = device
            .create_texture(
                &TextureDesc {
                    label: "texel",
                    size: UVec2::new(1, 1),
                    format: TextureFormat::Rgba8,
                },
                Some(&[51, 102, 153, 255]),
            )
            .unwrap();
        let target = device
            .create_render_target(UVec2::new(2, 2), TextureFormat::Rgba8)
            .unwrap();
        let counted = [
            "glUseProgram",
            "glBindVertexArray",
            "glVertexAttribPointer",
            "glBindTexture",
            "glBindSampler",
            "glBlendFuncSeparate",
            "glViewport",
            "glScissor",
        ];
        let counts = |device: &Device| counted.map(|call| device.gl().status().count(call));
        let frame = |device: &mut Device, draws: usize| {
            device.begin_frame(GpuSurface::Gl(&gl)).unwrap();
            let before = counts(device);
            {
                let clear = Some(LinearColor::BLACK);
                let mut pass = device.begin_pass(target.into(), clear).unwrap();
                pass.set_pipeline(pipeline).unwrap();
                pass.set_vertex_buffer(0, quad, 0).unwrap();
                pass.set_texture(0, texture, Sampler::NearestClamp).unwrap();
                for _ in 0..draws {
                    pass.draw(0..4, 0..1).unwrap();
                }
            }
            device.end_frame(GpuSurface::Gl(&gl)).unwrap();
            let after = counts(device);
            core::array::from_fn::<u32, 8, _>(|i| after[i] - before[i])
        };
        let once = frame(&mut device, 1);
        let twice = frame(&mut device, 2);
        // The second draw of a frame sets nothing again. The next frame sets
        // the context's state again, as it forgets the cache, but not the
        // attribute pointers: they are state of the vertex array, which
        // keeps them.
        let pointers = 2;
        assert_eq!((once[pointers], twice[pointers]), (1, 0), "{counted:?}");
        for (i, call) in counted.iter().enumerate() {
            if i != pointers {
                assert_eq!(once[i], twice[i], "{call}");
            }
        }
        assert!(
            once.iter().take(6).all(|&count| count >= 1),
            "{counted:?}: {once:?}"
        );
        assert_eq!(
            device.read_pixels(target).unwrap()[..4],
            [51, 102, 153, 255]
        );
    }

    /// A device, a 1 by 1 target, and a pipeline that draws a color from
    /// uniform block binding 0 over all of it.
    fn color_device(
        gl: &HeadlessGl,
    ) -> (
        Device,
        crate::RenderTargetId,
        crate::PipelineId,
        crate::BufferId,
    ) {
        const COLOR_VS: &str = "#version 330 core
layout(location = 0) in vec2 position;
void main() {
    gl_Position = vec4(position, 0.0, 1.0);
}
";
        const COLOR_FS: &str = "#version 330 core
layout(std140) uniform Color {
    vec4 color;
};
out vec4 out_color;
void main() {
    out_color = color;
}
";
        let mut device = Device::new(GpuSurface::Gl(gl)).unwrap();
        let attributes = [VertexAttribute {
            location: 0,
            format: VertexFormat::Float32x2,
            offset: 0,
        }];
        let layouts = [VertexLayout {
            stride: 8,
            step: VertexStep::Vertex,
            attributes: &attributes,
        }];
        let blocks = [crate::UniformBlockDesc {
            name: "Color",
            binding: 0,
            size: 16,
        }];
        let pipeline = device
            .create_pipeline(&PipelineDesc {
                label: "color",
                shader: ShaderSource {
                    glsl_vs: COLOR_VS,
                    glsl_fs: COLOR_FS,
                    msl: "",
                },
                vertex_layouts: &layouts,
                uniform_blocks: &blocks,
                textures: &[],
                blend: BlendMode::Opaque,
                topology: Topology::TriangleStrip,
                target_format: TextureFormat::Rgba8,
            })
            .unwrap();
        let quad = device
            .create_buffer(&BufferDesc {
                label: "quad",
                kind: BufferKind::Vertex,
                usage: BufferUsage::Static,
                size: 32,
            })
            .unwrap();
        device
            .write_buffer(quad, 0, &[-1.0f32, -1.0, 1.0, -1.0, -1.0, 1.0, 1.0, 1.0])
            .unwrap();
        let target = device
            .create_render_target(UVec2::new(1, 1), TextureFormat::Rgba8)
            .unwrap();
        (device, target, pipeline, quad)
    }

    #[test]
    fn in_release_a_call_that_failed_unseen_refuses_the_next_draw() {
        let Some(gl) = headless() else { return };
        let (mut device, target, pipeline, quad) = color_device(&gl);
        device.gl().status().set_every_call(false);
        device.begin_frame(GpuSurface::Gl(&gl)).unwrap();
        // Not checked in release, so it leaves its error for later.
        assert_eq!(device.gl().enable(0), Ok(()));
        {
            let mut pass = device.begin_pass(target.into(), None).unwrap();
            pass.set_pipeline(pipeline).unwrap();
            pass.set_vertex_buffer(0, quad, 0).unwrap();
            pass.set_uniforms(0, &[0.2f32, 0.4, 0.6, 1.0]).unwrap();
            assert!(matches!(
                pass.draw(0..4, 0..1),
                Err(GfxError::Gl {
                    code: nadir_sys::gl::GL_INVALID_ENUM,
                    ..
                })
            ));
            // The refused draw forgot what GL holds, so this one sets it all.
            pass.draw(0..4, 0..1).unwrap();
        }
        device.end_frame(GpuSurface::Gl(&gl)).unwrap();
        assert_eq!(device.read_pixels(target).unwrap(), [51, 102, 153, 255]);
    }

    #[test]
    fn in_release_refilling_the_uniform_ring_does_not_hide_an_earlier_error() {
        let Some(gl) = headless() else { return };
        let (mut device, target, pipeline, quad) = color_device(&gl);
        device.gl().status().set_every_call(false);
        device.begin_frame(GpuSurface::Gl(&gl)).unwrap();
        let draw = |device: &mut Device| {
            let mut pass = device.begin_pass(target.into(), None).unwrap();
            pass.set_pipeline(pipeline).unwrap();
            pass.set_vertex_buffer(0, quad, 0).unwrap();
            pass.set_uniforms(0, &[0.2f32, 0.4, 0.6, 1.0]).unwrap();
            pass.draw(0..4, 0..1)
        };
        // 256 blocks fill the 64 KiB ring at any stride the driver's
        // alignment gives (256 bytes and more, in powers of two).
        for _ in 0..256 {
            draw(&mut device).unwrap();
        }
        assert_eq!(device.gl().enable(0), Ok(()));
        // The next block needs new storage, whose allocation must not take
        // the error away from the check before the draw.
        assert!(matches!(draw(&mut device), Err(GfxError::Gl { .. })));
        draw(&mut device).unwrap();
        device.end_frame(GpuSurface::Gl(&gl)).unwrap();
        assert_eq!(device.read_pixels(target).unwrap(), [51, 102, 153, 255]);
    }
}
