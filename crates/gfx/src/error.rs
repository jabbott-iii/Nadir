//! The error type for `nadir-gfx`.

use std::fmt;

use nadir_core::UVec2;
use nadir_platform::PlatformError;
use nadir_sys::gl::{
    GL_FRAMEBUFFER_INCOMPLETE_ATTACHMENT, GL_FRAMEBUFFER_UNSUPPORTED, GL_INVALID_ENUM,
    GL_INVALID_FRAMEBUFFER_OPERATION, GL_INVALID_OPERATION, GL_INVALID_VALUE, GL_OUT_OF_MEMORY,
};

use crate::types::{ResourceKind, ShaderStage, TextureFormat};

/// Errors produced by `nadir-gfx`.
///
/// The errors that the draw path can return ([`Pass`](crate::Pass)'s
/// methods) hold no heap data, so reporting one allocates nothing.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GfxError {
    /// The surface is of a kind that this backend cannot draw with.
    UnsupportedSurface,
    /// The surface is not a current OpenGL context of `nadir-platform`'s:
    /// for instance the stand-in of a system that has none yet.
    NoContext,
    /// The device's OpenGL context is gone, or the driver ran out of memory
    /// before. The device can do nothing more.
    ContextLost,
    /// This thread already has a device.
    DeviceExists,
    /// The surface given to a frame is not the one the device was made for.
    WrongSurface,
    /// The driver lacks a function the backend calls.
    MissingFunction {
        /// The function, such as `glBindSampler`.
        name: &'static str,
    },
    /// The context is not OpenGL 3.3 or later with the core profile.
    GlTooOld {
        /// The driver's `GL_VERSION`.
        version: String,
    },
    /// A resource id was used after its resource was destroyed.
    StaleHandle(ResourceKind),
    /// A shader did not compile, or the program did not link.
    Shader {
        /// The pipeline's label.
        label: String,
        /// The stage that failed.
        stage: ShaderStage,
        /// What the driver said.
        log: String,
    },
    /// A pipeline description is not valid, or does not match its shaders.
    Interface {
        /// The pipeline's label.
        label: String,
        /// What is wrong.
        reason: String,
    },
    /// An argument is out of its range.
    InvalidArgument(&'static str),
    /// Some data is not the size it must be.
    DataSize {
        /// The bytes needed.
        expected: usize,
        /// The bytes given.
        found: usize,
    },
    /// A range reaches outside what it is a range of.
    OutOfBounds(&'static str),
    /// A texture or render target is larger than the device allows, or
    /// empty.
    TooLarge {
        /// The size asked for.
        size: UVec2,
        /// The largest side the device allows.
        max: u32,
    },
    /// The driver cannot draw into a render target of this kind.
    FramebufferIncomplete {
        /// What `glCheckFramebufferStatus` said.
        status: u32,
    },
    /// A frame or pass call came out of order, such as a pass outside a
    /// frame.
    FrameOrder(&'static str),
    /// A draw came before any pipeline was set.
    NoPipeline,
    /// A draw needs something that was not set in the pass.
    Unbound {
        /// What is missing.
        what: &'static str,
        /// Its slot or binding.
        slot: u32,
    },
    /// A pipeline was set in a pass whose target has another format.
    FormatMismatch {
        /// The format the pipeline draws to.
        pipeline: TextureFormat,
        /// The format of the pass's target.
        target: TextureFormat,
    },
    /// A pass would sample the render target it draws into.
    FeedbackLoop,
    /// The driver ran out of memory, and the device is then lost; or there
    /// was no memory for the device's copy of an index buffer.
    OutOfMemory(&'static str),
    /// An OpenGL call raised an error.
    Gl {
        /// The function that failed. When the error is seen later, as release
        /// builds see it, the last function called before it was seen.
        call: &'static str,
        /// The error code, such as `GL_INVALID_ENUM`.
        code: u32,
    },
    /// The frame could not be shown.
    Present(PlatformError),
}

impl fmt::Display for GfxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSurface => {
                f.write_str("the surface is of a kind this backend cannot use")
            }
            Self::NoContext => {
                f.write_str("the surface is not a current OpenGL context of nadir-platform's")
            }
            Self::ContextLost => f.write_str("the device's OpenGL context is lost"),
            Self::DeviceExists => f.write_str("this thread already has a graphics device"),
            Self::WrongSurface => {
                f.write_str("the surface is not the one the graphics device was made for")
            }
            Self::MissingFunction { name } => {
                write!(f, "the graphics driver does not have {name}")
            }
            Self::GlTooOld { version } => write!(
                f,
                "the graphics driver gave OpenGL `{version}`, not 3.3 or later (core profile)"
            ),
            Self::StaleHandle(kind) => write!(f, "the {} was destroyed", kind_name(*kind)),
            Self::Shader { label, stage, log } => {
                let stage = match stage {
                    ShaderStage::Vertex => "the vertex shader did not compile",
                    ShaderStage::Fragment => "the fragment shader did not compile",
                    ShaderStage::Link => "the shaders did not link",
                };
                write!(f, "pipeline `{label}`: {stage}: {log}")
            }
            Self::Interface { label, reason } => write!(f, "pipeline `{label}`: {reason}"),
            Self::InvalidArgument(what) => write!(f, "invalid argument: {what}"),
            Self::DataSize { expected, found } => {
                write!(f, "the data is {found} bytes, and must be {expected}")
            }
            Self::OutOfBounds(what) => write!(f, "out of bounds: {what}"),
            Self::TooLarge { size, max } => write!(
                f,
                "a texture of {}x{} pixels cannot be made: each side must be from 1 to {max}",
                size.x, size.y
            ),
            Self::FramebufferIncomplete { status } => write!(
                f,
                "the driver cannot draw into this render target: {}",
                framebuffer_status_name(*status)
            ),
            Self::FrameOrder(what) => write!(f, "out of order: {what}"),
            Self::NoPipeline => f.write_str("a draw came before any pipeline was set"),
            Self::Unbound { what, slot } => write!(f, "{what} {slot} was not set in this pass"),
            Self::FormatMismatch { pipeline, target } => write!(
                f,
                "the pipeline draws to {pipeline:?}, and the pass's target is {target:?}"
            ),
            Self::FeedbackLoop => {
                f.write_str("a pass cannot sample the render target it draws into")
            }
            Self::OutOfMemory(what) => write!(f, "the graphics driver ran out of memory: {what}"),
            Self::Gl { call, code } => write!(f, "{call} failed: {}", gl_error_name(*code)),
            Self::Present(error) => write!(f, "could not show the frame: {error}"),
        }
    }
}

impl std::error::Error for GfxError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Present(error) => Some(error),
            _ => None,
        }
    }
}

/// The name of a kind of resource, for messages.
fn kind_name(kind: ResourceKind) -> &'static str {
    match kind {
        ResourceKind::Texture => "texture",
        ResourceKind::Buffer => "buffer",
        ResourceKind::RenderTarget => "render target",
        ResourceKind::Pipeline => "pipeline",
    }
}

/// Names an OpenGL error code, for messages.
pub(crate) fn gl_error_name(code: u32) -> String {
    let name = match code {
        GL_INVALID_ENUM => "GL_INVALID_ENUM",
        GL_INVALID_VALUE => "GL_INVALID_VALUE",
        GL_INVALID_OPERATION => "GL_INVALID_OPERATION",
        GL_OUT_OF_MEMORY => "GL_OUT_OF_MEMORY",
        GL_INVALID_FRAMEBUFFER_OPERATION => "GL_INVALID_FRAMEBUFFER_OPERATION",
        _ => return format!("GL error {code:#06x}"),
    };
    name.to_string()
}

/// Names a framebuffer status, for messages.
fn framebuffer_status_name(status: u32) -> String {
    let name = match status {
        GL_FRAMEBUFFER_INCOMPLETE_ATTACHMENT => "GL_FRAMEBUFFER_INCOMPLETE_ATTACHMENT",
        GL_FRAMEBUFFER_UNSUPPORTED => "GL_FRAMEBUFFER_UNSUPPORTED",
        _ => return format!("status {status:#06x}"),
    };
    name.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_message_says_what_went_wrong() {
        let cases = [
            (
                GfxError::StaleHandle(ResourceKind::RenderTarget),
                "the render target was destroyed",
            ),
            (
                GfxError::MissingFunction {
                    name: "glBindSampler",
                },
                "the graphics driver does not have glBindSampler",
            ),
            (
                GfxError::Gl {
                    call: "glEnable",
                    code: 0x0500,
                },
                "glEnable failed: GL_INVALID_ENUM",
            ),
            (
                GfxError::Gl {
                    call: "glDrawArraysInstanced",
                    code: 0x0507,
                },
                "glDrawArraysInstanced failed: GL error 0x0507",
            ),
            (
                GfxError::FramebufferIncomplete { status: 0x8CD6 },
                "the driver cannot draw into this render target: \
                 GL_FRAMEBUFFER_INCOMPLETE_ATTACHMENT",
            ),
            (
                GfxError::FramebufferIncomplete { status: 0x8CD7 },
                "the driver cannot draw into this render target: status 0x8cd7",
            ),
            (
                GfxError::Unbound {
                    what: "uniform block",
                    slot: 2,
                },
                "uniform block 2 was not set in this pass",
            ),
            (
                GfxError::TooLarge {
                    size: UVec2::new(40000, 1),
                    max: 16384,
                },
                "a texture of 40000x1 pixels cannot be made: each side must be from 1 to 16384",
            ),
        ];
        for (error, message) in cases {
            assert_eq!(error.to_string(), message);
        }
    }

    #[test]
    fn a_shader_error_shows_the_label_the_stage_and_the_log() {
        let error = GfxError::Shader {
            label: "sprite".to_string(),
            stage: ShaderStage::Fragment,
            log: "0:3(5): error: `undefined_thing' undeclared".to_string(),
        };
        assert_eq!(
            error.to_string(),
            "pipeline `sprite`: the fragment shader did not compile: \
             0:3(5): error: `undefined_thing' undeclared"
        );
    }

    #[test]
    fn a_present_error_has_its_cause_as_source() {
        let error = GfxError::Present(PlatformError::Unsupported);
        let source = std::error::Error::source(&error).map(ToString::to_string);
        assert_eq!(
            source.as_deref(),
            Some("this operating system has no backend yet")
        );
        assert!(std::error::Error::source(&GfxError::NoPipeline).is_none());
    }
}
