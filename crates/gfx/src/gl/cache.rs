//! What the backend last set in GL, so that it can skip calls that would
//! change nothing.
//!
//! Each value is the GL name or setting last made, or `None` when it is
//! unknown. The cache forgets everything at the start of a frame and when a
//! resource is destroyed: deleting a bound object unbinds it, and GL then
//! hands its name out again. A value is recorded only once its call
//! succeeded.

use nadir_sys::gl::{
    GL_ARRAY_BUFFER, GL_BLEND, GL_DST_COLOR, GL_ONE, GL_ONE_MINUS_SRC_ALPHA, GL_SCISSOR_TEST,
    GL_SRC_ALPHA, GL_UNIFORM_BUFFER, GL_ZERO, GLenum, GLuint,
};

use super::calls::Gl;
use crate::GfxError;
use crate::types::{BlendMode, MAX_TEXTURE_SLOTS};

/// The number of texture units the cache follows.
const UNITS: usize = MAX_TEXTURE_SLOTS as usize;

/// The factors of a blend mode, for `glBlendFuncSeparate`: source and
/// destination of the colors, then of alpha. `None` turns blending off.
pub(crate) fn blend_factors(mode: BlendMode) -> Option<[GLenum; 4]> {
    match mode {
        BlendMode::Opaque => None,
        BlendMode::Alpha => Some([
            GL_SRC_ALPHA,
            GL_ONE_MINUS_SRC_ALPHA,
            GL_ONE,
            GL_ONE_MINUS_SRC_ALPHA,
        ]),
        BlendMode::PremultipliedAlpha => Some([
            GL_ONE,
            GL_ONE_MINUS_SRC_ALPHA,
            GL_ONE,
            GL_ONE_MINUS_SRC_ALPHA,
        ]),
        BlendMode::Additive => Some([GL_SRC_ALPHA, GL_ONE, GL_ZERO, GL_ONE]),
        BlendMode::Multiply => Some([GL_DST_COLOR, GL_ZERO, GL_ZERO, GL_ONE]),
    }
}

#[derive(Default)]
pub(crate) struct Cache {
    program: Option<GLuint>,
    vertex_array: Option<GLuint>,
    framebuffer: Option<GLuint>,
    array_buffer: Option<GLuint>,
    uniform_buffer: Option<GLuint>,
    active_unit: Option<u32>,
    unit_texture: [Option<GLuint>; UNITS],
    unit_sampler: [Option<GLuint>; UNITS],
    blending: Option<bool>,
    blend: Option<[GLenum; 4]>,
    viewport: Option<[i32; 4]>,
    scissor_test: Option<bool>,
    scissor: Option<[i32; 4]>,
}

impl Cache {
    /// Forgets everything.
    pub(crate) fn invalidate(&mut self) {
        *self = Self::default();
    }

    pub(crate) fn use_program(&mut self, gl: Gl<'_>, program: GLuint) -> Result<(), GfxError> {
        if self.program != Some(program) {
            self.program = None;
            gl.use_program(program)?;
            self.program = Some(program);
        }
        Ok(())
    }

    /// Binds a vertex array. Its element buffer comes with it.
    pub(crate) fn bind_vertex_array(&mut self, gl: Gl<'_>, array: GLuint) -> Result<(), GfxError> {
        if self.vertex_array != Some(array) {
            self.vertex_array = None;
            gl.bind_vertex_array(array)?;
            self.vertex_array = Some(array);
        }
        Ok(())
    }

    pub(crate) fn bind_framebuffer(
        &mut self,
        gl: Gl<'_>,
        framebuffer: GLuint,
    ) -> Result<(), GfxError> {
        if self.framebuffer != Some(framebuffer) {
            self.framebuffer = None;
            gl.bind_framebuffer(framebuffer)?;
            self.framebuffer = Some(framebuffer);
        }
        Ok(())
    }

    pub(crate) fn bind_array_buffer(&mut self, gl: Gl<'_>, buffer: GLuint) -> Result<(), GfxError> {
        if self.array_buffer != Some(buffer) {
            self.array_buffer = None;
            gl.bind_buffer(GL_ARRAY_BUFFER, buffer)?;
            self.array_buffer = Some(buffer);
        }
        Ok(())
    }

    pub(crate) fn bind_uniform_buffer(
        &mut self,
        gl: Gl<'_>,
        buffer: GLuint,
    ) -> Result<(), GfxError> {
        if self.uniform_buffer != Some(buffer) {
            self.uniform_buffer = None;
            gl.bind_buffer(GL_UNIFORM_BUFFER, buffer)?;
            self.uniform_buffer = Some(buffer);
        }
        Ok(())
    }

    /// Binds a range of `buffer` to uniform block binding `binding`, which
    /// also binds it to `GL_UNIFORM_BUFFER`.
    pub(crate) fn bind_uniform_range(
        &mut self,
        gl: Gl<'_>,
        binding: u32,
        buffer: GLuint,
        offset: usize,
        size: usize,
    ) -> Result<(), GfxError> {
        self.uniform_buffer = None;
        gl.bind_buffer_range(GL_UNIFORM_BUFFER, binding, buffer, offset, size)?;
        self.uniform_buffer = Some(buffer);
        Ok(())
    }

    fn activate(&mut self, gl: Gl<'_>, unit: u32) -> Result<(), GfxError> {
        if self.active_unit != Some(unit) {
            self.active_unit = None;
            gl.active_texture(unit)?;
            self.active_unit = Some(unit);
        }
        Ok(())
    }

    /// Binds a texture to unit `unit`, which is below [`UNITS`].
    pub(crate) fn bind_texture(
        &mut self,
        gl: Gl<'_>,
        unit: u32,
        texture: GLuint,
    ) -> Result<(), GfxError> {
        let slot = unit as usize;
        if self.unit_texture.get(slot).copied().flatten() == Some(texture) {
            return Ok(());
        }
        self.activate(gl, unit)?;
        if let Some(bound) = self.unit_texture.get_mut(slot) {
            *bound = None;
            gl.bind_texture(texture)?;
            *bound = Some(texture);
        }
        Ok(())
    }

    /// Binds a texture to change it, on whichever unit is active.
    pub(crate) fn bind_texture_to_change(
        &mut self,
        gl: Gl<'_>,
        texture: GLuint,
    ) -> Result<(), GfxError> {
        let unit = match self.active_unit {
            Some(unit) => unit,
            None => {
                self.activate(gl, 0)?;
                0
            }
        };
        self.bind_texture(gl, unit, texture)
    }

    pub(crate) fn bind_sampler(
        &mut self,
        gl: Gl<'_>,
        unit: u32,
        sampler: GLuint,
    ) -> Result<(), GfxError> {
        if let Some(bound) = self.unit_sampler.get_mut(unit as usize)
            && *bound != Some(sampler)
        {
            *bound = None;
            gl.bind_sampler(unit, sampler)?;
            *bound = Some(sampler);
        }
        Ok(())
    }

    pub(crate) fn blend(&mut self, gl: Gl<'_>, mode: BlendMode) -> Result<(), GfxError> {
        let factors = blend_factors(mode);
        let on = factors.is_some();
        if self.blending != Some(on) {
            self.blending = None;
            if on {
                gl.enable(GL_BLEND)?;
            } else {
                gl.disable(GL_BLEND)?;
            }
            self.blending = Some(on);
        }
        if let Some(factors) = factors
            && self.blend != Some(factors)
        {
            self.blend = None;
            gl.blend_func_separate(factors)?;
            self.blend = Some(factors);
        }
        Ok(())
    }

    pub(crate) fn viewport(&mut self, gl: Gl<'_>, viewport: [i32; 4]) -> Result<(), GfxError> {
        if self.viewport != Some(viewport) {
            self.viewport = None;
            gl.viewport(viewport)?;
            self.viewport = Some(viewport);
        }
        Ok(())
    }

    /// Turns the scissor test on with `scissor`, or off.
    pub(crate) fn scissor(
        &mut self,
        gl: Gl<'_>,
        scissor: Option<[i32; 4]>,
    ) -> Result<(), GfxError> {
        let on = scissor.is_some();
        if self.scissor_test != Some(on) {
            self.scissor_test = None;
            if on {
                gl.enable(GL_SCISSOR_TEST)?;
            } else {
                gl.disable(GL_SCISSOR_TEST)?;
            }
            self.scissor_test = Some(on);
        }
        if let Some(scissor) = scissor
            && self.scissor != Some(scissor)
        {
            self.scissor = None;
            gl.scissor(scissor)?;
            self.scissor = Some(scissor);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_blend_mode_has_its_factors() {
        assert_eq!(blend_factors(BlendMode::Opaque), None);
        assert_eq!(
            blend_factors(BlendMode::Alpha),
            Some([
                GL_SRC_ALPHA,
                GL_ONE_MINUS_SRC_ALPHA,
                GL_ONE,
                GL_ONE_MINUS_SRC_ALPHA
            ])
        );
        assert_eq!(
            blend_factors(BlendMode::PremultipliedAlpha),
            Some([
                GL_ONE,
                GL_ONE_MINUS_SRC_ALPHA,
                GL_ONE,
                GL_ONE_MINUS_SRC_ALPHA
            ])
        );
        assert_eq!(
            blend_factors(BlendMode::Additive),
            Some([GL_SRC_ALPHA, GL_ONE, GL_ZERO, GL_ONE])
        );
        assert_eq!(
            blend_factors(BlendMode::Multiply),
            Some([GL_DST_COLOR, GL_ZERO, GL_ZERO, GL_ONE])
        );
    }
}
