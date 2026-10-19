//! Calls into OpenGL: one safe wrapper for each function, and the token that
//! proves a call may be made.
//!
//! Each `unsafe` block holds one operation: the call, or, in a wrapper that
//! reads what GL returns, the read. What makes each one sound is said once,
//! on [`Gl`], and each block adds what is particular to its function: chiefly
//! that the pointers it passes cover what GL reads or writes. Only the two
//! draws stay `unsafe`: what they read depends on state that only the caller
//! knows to be in bounds.
//!
//! # Errors
//!
//! After each call, a wrapper records the function's name. In debug builds it
//! also reads `glGetError`, and turns an error into [`GfxError::Gl`]. Release
//! builds leave the errors for [`Gl::check_pending`], before each draw, and
//! for the end of the frame. The calls that allocate (`glBufferData`,
//! `glTexImage2D`) are checked in every build, and fail if an earlier call
//! left an error: none is ever dropped unseen. Out of memory, wherever it is
//! seen, loses the device.

use core::cell::Cell;
use core::ffi::{CStr, c_char, c_void};
use core::ptr;

use nadir_platform::GlContextId;
use nadir_sys::gl::{
    GL_ACTIVE_UNIFORM_BLOCKS, GL_COLOR_ATTACHMENT0, GL_COMPILE_STATUS, GL_CONTEXT_PROFILE_MASK,
    GL_FALSE, GL_FLOAT, GL_FRAMEBUFFER, GL_INFO_LOG_LENGTH, GL_INVALID_VALUE, GL_LINK_STATUS,
    GL_MAJOR_VERSION, GL_MAX_TEXTURE_SIZE, GL_MINOR_VERSION, GL_NO_ERROR, GL_OUT_OF_MEMORY,
    GL_RENDERER, GL_RGBA, GL_TEXTURE_2D, GL_TEXTURE0, GL_UNIFORM_BLOCK_DATA_SIZE,
    GL_UNIFORM_BUFFER_OFFSET_ALIGNMENT, GL_UNPACK_ALIGNMENT, GL_UNSIGNED_BYTE, GL_VENDOR,
    GL_VERSION, GLbitfield, GLchar, GLenum, GLfloat, GLint, GLintptr, GLsizei, GLsizeiptr, GLuint,
};

use super::fns::GlFns;
use crate::GfxError;
use crate::validate::trim_log;

/// What the calls of a device have in common: whether it is lost, and how
/// errors are checked.
pub(crate) struct Status {
    /// The device can make no more calls: the context is gone, or GL ran
    /// out of memory.
    lost: Cell<bool>,
    /// The last function called, for the once-a-frame error report.
    last_call: Cell<&'static str>,
    /// Whether every call is checked with `glGetError`.
    every_call: Cell<bool>,
    /// How many times each function was called, for the tests of the state
    /// cache.
    #[cfg(test)]
    calls: core::cell::RefCell<nadir_core::FastHashMap<&'static str, u32>>,
}

impl Status {
    pub(crate) fn new() -> Self {
        Self {
            lost: Cell::new(false),
            last_call: Cell::new("(none)"),
            every_call: Cell::new(cfg!(debug_assertions)),
            #[cfg(test)]
            calls: core::cell::RefCell::default(),
        }
    }

    /// The last function called.
    pub(crate) fn last_call(&self) -> &'static str {
        self.last_call.get()
    }

    /// Checks every call, or only once a frame.
    #[cfg(all(test, feature = "headless-test"))]
    pub(crate) fn set_every_call(&self, every_call: bool) {
        self.every_call.set(every_call);
    }

    /// Whether every call is checked.
    #[cfg(all(test, feature = "headless-test"))]
    pub(crate) fn every_call(&self) -> bool {
        self.every_call.get()
    }

    /// How many times `call` was called.
    #[cfg(all(test, feature = "headless-test"))]
    pub(crate) fn count(&self, call: &str) -> u32 {
        self.calls.borrow().get(call).copied().unwrap_or(0)
    }
}

/// The right to call OpenGL: a context of `nadir-platform`'s is current on
/// this thread, and its functions are in `fns`.
///
/// Every `unsafe` block in this file relies on it. Only [`Gl::new`] makes
/// one, after it saw the device's context id current on this thread. By
/// `GlContext`'s promise, the functions loaded from that context stay
/// callable for as long as the id is current, and the id stays current
/// until the context is dropped. A token lives only within one call into
/// `nadir-gfx`, which runs no code of its callers' and drops no context, so
/// the context outlives the token.
#[derive(Clone, Copy)]
pub(crate) struct Gl<'a> {
    fns: &'a GlFns,
    status: &'a Status,
}

/// An integer that `glGetIntegerv` writes one of.
#[derive(Clone, Copy)]
pub(crate) enum Integer {
    MajorVersion,
    MinorVersion,
    ProfileMask,
    MaxTextureSize,
    UniformBufferOffsetAlignment,
}

/// A string that `glGetString` gives.
#[derive(Clone, Copy)]
pub(crate) enum Text {
    Vendor,
    Renderer,
    Version,
}

impl<'a> Gl<'a> {
    /// The token, if `context` is current on this thread and the device is
    /// not lost.
    pub(crate) fn new(
        fns: &'a GlFns,
        status: &'a Status,
        context: GlContextId,
    ) -> Result<Self, GfxError> {
        if status.lost.get() || !context.is_current() {
            return Err(GfxError::ContextLost);
        }
        Ok(Self { fns, status })
    }

    /// The token for deleting objects: the context is current, whether the
    /// device is lost or not.
    pub(crate) fn for_teardown(
        fns: &'a GlFns,
        status: &'a Status,
        context: GlContextId,
    ) -> Option<Self> {
        context.is_current().then_some(Self { fns, status })
    }

    /// Records that `call` was made, and in debug builds checks it.
    fn after(self, call: &'static str) -> Result<(), GfxError> {
        self.status.last_call.set(call);
        #[cfg(test)]
        {
            *self.status.calls.borrow_mut().entry(call).or_insert(0) += 1;
        }
        if !self.status.every_call.get() {
            return Ok(());
        }
        match self.drain_errors() {
            None => Ok(()),
            Some(code) => Err(GfxError::Gl { call, code }),
        }
    }

    /// Takes one error that GL has set, if any.
    fn take_error(self) -> Option<GLenum> {
        // SAFETY: see `Gl`. `glGetError` takes nothing.
        let code = unsafe { (self.fns.get_error)() };
        (code != GL_NO_ERROR).then_some(code)
    }

    /// Takes every error that GL has set, and returns the first, or
    /// `GL_OUT_OF_MEMORY` if it is among them, which loses the device. A
    /// driver keeps a few error flags, so a few reads clear them.
    pub(crate) fn drain_errors(self) -> Option<GLenum> {
        let first = self.take_error()?;
        let mut out_of_memory = first == GL_OUT_OF_MEMORY;
        for _ in 0..16 {
            match self.take_error() {
                None => break,
                Some(code) => out_of_memory |= code == GL_OUT_OF_MEMORY,
            }
        }
        if out_of_memory {
            self.status.lost.set(true);
            return Some(GL_OUT_OF_MEMORY);
        }
        Some(first)
    }

    /// Fails if an earlier call left an error. In release builds, where
    /// calls are not checked one by one, the error names the last call made
    /// before it was seen.
    pub(crate) fn check_pending(self, what: &'static str) -> Result<(), GfxError> {
        match self.drain_errors() {
            None => Ok(()),
            Some(GL_OUT_OF_MEMORY) => Err(GfxError::OutOfMemory(what)),
            Some(code) => Err(GfxError::Gl {
                call: self.status.last_call.get(),
                code,
            }),
        }
    }

    /// After a call that allocates: the result of the check that every build
    /// makes. Out of memory loses the device.
    fn after_allocating(self, call: &'static str, what: &'static str) -> Result<(), GfxError> {
        self.status.last_call.set(call);
        #[cfg(test)]
        {
            *self.status.calls.borrow_mut().entry(call).or_insert(0) += 1;
        }
        match self.drain_errors() {
            None => Ok(()),
            Some(GL_OUT_OF_MEMORY) => Err(GfxError::OutOfMemory(what)),
            Some(GL_INVALID_VALUE) => Err(GfxError::InvalidArgument(what)),
            Some(code) => Err(GfxError::Gl { call, code }),
        }
    }

    /// A name from a `glGen*` or `glCreate*` call, unless it is 0, which no
    /// object has: the driver could make none, and the device is lost.
    fn named(self, name: GLuint, what: &'static str) -> Result<GLuint, GfxError> {
        if name == 0 {
            self.status.lost.set(true);
            return Err(GfxError::OutOfMemory(what));
        }
        Ok(name)
    }

    // -- State and queries ----------------------------------------------------

    pub(crate) fn get_integer(self, name: Integer) -> Result<GLint, GfxError> {
        let pname = match name {
            Integer::MajorVersion => GL_MAJOR_VERSION,
            Integer::MinorVersion => GL_MINOR_VERSION,
            Integer::ProfileMask => GL_CONTEXT_PROFILE_MASK,
            Integer::MaxTextureSize => GL_MAX_TEXTURE_SIZE,
            Integer::UniformBufferOffsetAlignment => GL_UNIFORM_BUFFER_OFFSET_ALIGNMENT,
        };
        // A version that does not know the name leaves the value alone.
        let mut value = 0;
        // SAFETY: see `Gl`. Each of these names writes one integer, to a live
        // `GLint`.
        unsafe { (self.fns.get_integerv)(pname, &raw mut value) };
        self.after("glGetIntegerv")?;
        Ok(value)
    }

    /// The string, or an empty one if GL gives none.
    pub(crate) fn get_string(self, name: Text) -> Result<String, GfxError> {
        let name = match name {
            Text::Vendor => GL_VENDOR,
            Text::Renderer => GL_RENDERER,
            Text::Version => GL_VERSION,
        };
        // SAFETY: see `Gl`. `glGetString` takes a value.
        let text = unsafe { (self.fns.get_string)(name) };
        self.after("glGetString")?;
        if text.is_null() {
            return Ok(String::new());
        }
        // SAFETY: GL returned a NUL-terminated string, which lives as long as
        // the context. It is copied here.
        let text = unsafe { CStr::from_ptr(text.cast::<c_char>()) };
        Ok(text.to_string_lossy().into_owned())
    }

    pub(crate) fn enable(self, cap: GLenum) -> Result<(), GfxError> {
        // SAFETY: see `Gl`. `glEnable` takes a value.
        unsafe { (self.fns.enable)(cap) };
        self.after("glEnable")
    }

    pub(crate) fn disable(self, cap: GLenum) -> Result<(), GfxError> {
        // SAFETY: see `Gl`. `glDisable` takes a value.
        unsafe { (self.fns.disable)(cap) };
        self.after("glDisable")
    }

    pub(crate) fn viewport(self, [x, y, width, height]: [GLint; 4]) -> Result<(), GfxError> {
        // SAFETY: see `Gl`. `glViewport` takes values.
        unsafe { (self.fns.viewport)(x, y, width, height) };
        self.after("glViewport")
    }

    pub(crate) fn scissor(self, [x, y, width, height]: [GLint; 4]) -> Result<(), GfxError> {
        // SAFETY: see `Gl`. `glScissor` takes values.
        unsafe { (self.fns.scissor)(x, y, width, height) };
        self.after("glScissor")
    }

    pub(crate) fn clear_color(self, [r, g, b, a]: [GLfloat; 4]) -> Result<(), GfxError> {
        // SAFETY: see `Gl`. `glClearColor` takes values.
        unsafe { (self.fns.clear_color)(r, g, b, a) };
        self.after("glClearColor")
    }

    pub(crate) fn clear(self, mask: GLbitfield) -> Result<(), GfxError> {
        // SAFETY: see `Gl`. `glClear` takes a value.
        unsafe { (self.fns.clear)(mask) };
        self.after("glClear")
    }

    pub(crate) fn blend_func_separate(
        self,
        [src, dst, src_a, dst_a]: [GLenum; 4],
    ) -> Result<(), GfxError> {
        // SAFETY: see `Gl`. `glBlendFuncSeparate` takes values.
        unsafe { (self.fns.blend_func_separate)(src, dst, src_a, dst_a) };
        self.after("glBlendFuncSeparate")
    }

    /// Sets `GL_UNPACK_ALIGNMENT` to 1: rows of uploaded pixels are tightly
    /// packed. The texture wrappers rely on it.
    pub(crate) fn unpack_tightly(self) -> Result<(), GfxError> {
        // SAFETY: see `Gl`. `glPixelStorei` takes values.
        unsafe { (self.fns.pixel_storei)(GL_UNPACK_ALIGNMENT, 1) };
        self.after("glPixelStorei")
    }

    // -- Textures -------------------------------------------------------------

    pub(crate) fn gen_texture(self) -> Result<GLuint, GfxError> {
        let mut name = 0;
        // SAFETY: see `Gl`. One name goes to a live `GLuint`.
        unsafe { (self.fns.gen_textures)(1, &raw mut name) };
        self.after("glGenTextures")?;
        self.named(name, "glGenTextures gave no name")
    }

    pub(crate) fn delete_texture(self, name: GLuint) -> Result<(), GfxError> {
        // SAFETY: see `Gl`. One name is read from a live `GLuint`.
        unsafe { (self.fns.delete_textures)(1, &raw const name) };
        self.after("glDeleteTextures")
    }

    /// Makes texture unit `unit` the one that texture calls change.
    pub(crate) fn active_texture(self, unit: u32) -> Result<(), GfxError> {
        // SAFETY: see `Gl`. `glActiveTexture` takes a value.
        unsafe { (self.fns.active_texture)(GL_TEXTURE0 + unit) };
        self.after("glActiveTexture")
    }

    pub(crate) fn bind_texture(self, name: GLuint) -> Result<(), GfxError> {
        // SAFETY: see `Gl`. `glBindTexture` takes values.
        unsafe { (self.fns.bind_texture)(GL_TEXTURE_2D, name) };
        self.after("glBindTexture")
    }

    pub(crate) fn tex_parameter(self, pname: GLenum, value: GLint) -> Result<(), GfxError> {
        // SAFETY: see `Gl`. `glTexParameteri` takes values.
        unsafe { (self.fns.tex_parameteri)(GL_TEXTURE_2D, pname, value) };
        self.after("glTexParameteri")
    }

    /// Gives the bound 2D texture storage of `width` by `height`, filled from
    /// `pixels` if there are some: tightly packed rows of `bytes_per_pixel`
    /// bytes a pixel, in the `format` of `GL_UNSIGNED_BYTE`s.
    pub(crate) fn tex_image_2d(
        self,
        internal_format: GLint,
        [width, height]: [u32; 2],
        format: GLenum,
        bytes_per_pixel: usize,
        pixels: Option<&[u8]>,
    ) -> Result<(), GfxError> {
        let (w, h) = (sizei(width)?, sizei(height)?);
        if let Some(pixels) = pixels {
            let expected = image_bytes(width, height, bytes_per_pixel)?;
            if pixels.len() != expected {
                return Err(GfxError::DataSize {
                    expected,
                    found: pixels.len(),
                });
            }
        }
        let pointer = pixels.map_or(ptr::null(), |pixels| pixels.as_ptr().cast::<c_void>());
        self.check_pending("a texture")?;
        // SAFETY: see `Gl`. With rows tightly packed (`unpack_tightly`, which
        // the device sets before any upload and nothing changes, and no
        // unpack buffer bound), GL reads `width * height * bytes_per_pixel`
        // bytes, which `pixels` holds, or nothing if it is null.
        unsafe {
            (self.fns.tex_image_2d)(
                GL_TEXTURE_2D,
                0,
                internal_format,
                w,
                h,
                0,
                format,
                GL_UNSIGNED_BYTE,
                pointer,
            )
        };
        self.after_allocating("glTexImage2D", "a texture")
    }

    /// Replaces the texels of `[x, y, width, height]` (from the first row of
    /// the texture) in the bound 2D texture with `pixels`, tightly packed.
    pub(crate) fn tex_sub_image_2d(
        self,
        [x, y, width, height]: [u32; 4],
        format: GLenum,
        bytes_per_pixel: usize,
        pixels: &[u8],
    ) -> Result<(), GfxError> {
        let expected = image_bytes(width, height, bytes_per_pixel)?;
        if pixels.len() != expected {
            return Err(GfxError::DataSize {
                expected,
                found: pixels.len(),
            });
        }
        let (x, y, w, h) = (int(x)?, int(y)?, sizei(width)?, sizei(height)?);
        // SAFETY: see `Gl`. With rows tightly packed (as for `tex_image_2d`),
        // GL reads `width * height * bytes_per_pixel` bytes, which `pixels`
        // holds.
        unsafe {
            (self.fns.tex_sub_image_2d)(
                GL_TEXTURE_2D,
                0,
                x,
                y,
                w,
                h,
                format,
                GL_UNSIGNED_BYTE,
                pixels.as_ptr().cast::<c_void>(),
            )
        };
        self.after("glTexSubImage2D")
    }

    // -- Samplers -------------------------------------------------------------

    pub(crate) fn gen_sampler(self) -> Result<GLuint, GfxError> {
        let mut name = 0;
        // SAFETY: see `Gl`. One name goes to a live `GLuint`.
        unsafe { (self.fns.gen_samplers)(1, &raw mut name) };
        self.after("glGenSamplers")?;
        self.named(name, "glGenSamplers gave no name")
    }

    pub(crate) fn delete_sampler(self, name: GLuint) -> Result<(), GfxError> {
        // SAFETY: see `Gl`. One name is read from a live `GLuint`.
        unsafe { (self.fns.delete_samplers)(1, &raw const name) };
        self.after("glDeleteSamplers")
    }

    pub(crate) fn bind_sampler(self, unit: u32, name: GLuint) -> Result<(), GfxError> {
        // SAFETY: see `Gl`. `glBindSampler` takes values.
        unsafe { (self.fns.bind_sampler)(unit, name) };
        self.after("glBindSampler")
    }

    pub(crate) fn sampler_parameter(
        self,
        sampler: GLuint,
        pname: GLenum,
        value: GLint,
    ) -> Result<(), GfxError> {
        // SAFETY: see `Gl`. `glSamplerParameteri` takes values.
        unsafe { (self.fns.sampler_parameteri)(sampler, pname, value) };
        self.after("glSamplerParameteri")
    }

    // -- Buffers --------------------------------------------------------------

    pub(crate) fn gen_buffer(self) -> Result<GLuint, GfxError> {
        let mut name = 0;
        // SAFETY: see `Gl`. One name goes to a live `GLuint`.
        unsafe { (self.fns.gen_buffers)(1, &raw mut name) };
        self.after("glGenBuffers")?;
        self.named(name, "glGenBuffers gave no name")
    }

    pub(crate) fn delete_buffer(self, name: GLuint) -> Result<(), GfxError> {
        // SAFETY: see `Gl`. One name is read from a live `GLuint`.
        unsafe { (self.fns.delete_buffers)(1, &raw const name) };
        self.after("glDeleteBuffers")
    }

    pub(crate) fn bind_buffer(self, target: GLenum, name: GLuint) -> Result<(), GfxError> {
        // SAFETY: see `Gl`. `glBindBuffer` takes values.
        unsafe { (self.fns.bind_buffer)(target, name) };
        self.after("glBindBuffer")
    }

    /// Gives the buffer bound to `target` new storage of `size` bytes,
    /// filled from `data` if there is some.
    pub(crate) fn buffer_data(
        self,
        target: GLenum,
        size: usize,
        data: Option<&[u8]>,
        usage: GLenum,
        what: &'static str,
    ) -> Result<(), GfxError> {
        if let Some(data) = data
            && data.len() != size
        {
            return Err(GfxError::DataSize {
                expected: size,
                found: data.len(),
            });
        }
        let length =
            GLsizeiptr::try_from(size).map_err(|_| GfxError::InvalidArgument("a buffer size"))?;
        let pointer = data.map_or(ptr::null(), |data| data.as_ptr().cast::<c_void>());
        self.check_pending(what)?;
        // SAFETY: see `Gl`. GL reads `size` bytes from `data`, which holds
        // them, or nothing if it is null, and copies them before it returns.
        unsafe { (self.fns.buffer_data)(target, length, pointer, usage) };
        self.after_allocating("glBufferData", what)
    }

    /// Writes `data` into the buffer bound to `target`, from byte `offset`.
    /// GL refuses a range outside the buffer.
    pub(crate) fn buffer_sub_data(
        self,
        target: GLenum,
        offset: usize,
        data: &[u8],
    ) -> Result<(), GfxError> {
        let offset =
            GLintptr::try_from(offset).map_err(|_| GfxError::OutOfBounds("a buffer offset"))?;
        let length = GLsizeiptr::try_from(data.len())
            .map_err(|_| GfxError::OutOfBounds("a buffer write"))?;
        // SAFETY: see `Gl`. GL reads `data.len()` bytes from `data`, and
        // copies them before it returns.
        unsafe {
            (self.fns.buffer_sub_data)(target, offset, length, data.as_ptr().cast::<c_void>())
        };
        self.after("glBufferSubData")
    }

    /// [`Gl::buffer_sub_data`], checked in every build: it fails if GL
    /// reports any error for it.
    pub(crate) fn buffer_sub_data_checked(
        self,
        target: GLenum,
        offset: usize,
        data: &[u8],
    ) -> Result<(), GfxError> {
        self.check_pending("a buffer write")?;
        let every_call = self.status.every_call.replace(false);
        let written = self.buffer_sub_data(target, offset, data);
        self.status.every_call.set(every_call);
        written?;
        self.after_allocating("glBufferSubData", "a buffer write")
    }

    /// Binds `size` bytes of `buffer` from byte `offset` to the indexed
    /// binding `index` of `target`. It also binds the buffer to `target`.
    pub(crate) fn bind_buffer_range(
        self,
        target: GLenum,
        index: u32,
        buffer: GLuint,
        offset: usize,
        size: usize,
    ) -> Result<(), GfxError> {
        let offset =
            GLintptr::try_from(offset).map_err(|_| GfxError::OutOfBounds("a buffer offset"))?;
        let size =
            GLsizeiptr::try_from(size).map_err(|_| GfxError::OutOfBounds("a buffer range"))?;
        // SAFETY: see `Gl`. `glBindBufferRange` takes values.
        unsafe { (self.fns.bind_buffer_range)(target, index, buffer, offset, size) };
        self.after("glBindBufferRange")
    }

    // -- Vertex arrays ----------------------------------------------------------

    pub(crate) fn gen_vertex_array(self) -> Result<GLuint, GfxError> {
        let mut name = 0;
        // SAFETY: see `Gl`. One name goes to a live `GLuint`.
        unsafe { (self.fns.gen_vertex_arrays)(1, &raw mut name) };
        self.after("glGenVertexArrays")?;
        self.named(name, "glGenVertexArrays gave no name")
    }

    pub(crate) fn delete_vertex_array(self, name: GLuint) -> Result<(), GfxError> {
        // SAFETY: see `Gl`. One name is read from a live `GLuint`.
        unsafe { (self.fns.delete_vertex_arrays)(1, &raw const name) };
        self.after("glDeleteVertexArrays")
    }

    pub(crate) fn bind_vertex_array(self, name: GLuint) -> Result<(), GfxError> {
        // SAFETY: see `Gl`. `glBindVertexArray` takes a value.
        unsafe { (self.fns.bind_vertex_array)(name) };
        self.after("glBindVertexArray")
    }

    pub(crate) fn enable_vertex_attrib_array(self, location: u32) -> Result<(), GfxError> {
        // SAFETY: see `Gl`. `glEnableVertexAttribArray` takes a value.
        unsafe { (self.fns.enable_vertex_attrib_array)(location) };
        self.after("glEnableVertexAttribArray")
    }

    /// Points attribute `location` of the bound vertex array at `components`
    /// `f32`s, from byte `offset` of the buffer bound to `GL_ARRAY_BUFFER`,
    /// one record every `stride` bytes.
    pub(crate) fn vertex_attrib_pointer(
        self,
        location: u32,
        components: u32,
        stride: u32,
        offset: usize,
    ) -> Result<(), GfxError> {
        let components = int(components)?;
        let stride = sizei(stride)?;
        // The "pointer" is an offset into the bound buffer, which GL does not
        // follow until a draw reads the attribute.
        let offset = ptr::without_provenance::<c_void>(offset);
        // SAFETY: see `Gl`. With a buffer bound, `glVertexAttribPointer`
        // reads nothing through the offset: it records it.
        unsafe {
            (self.fns.vertex_attrib_pointer)(
                location, components, GL_FLOAT, GL_FALSE, stride, offset,
            )
        };
        self.after("glVertexAttribPointer")
    }

    pub(crate) fn vertex_attrib_divisor(self, location: u32, divisor: u32) -> Result<(), GfxError> {
        // SAFETY: see `Gl`. `glVertexAttribDivisor` takes values.
        unsafe { (self.fns.vertex_attrib_divisor)(location, divisor) };
        self.after("glVertexAttribDivisor")
    }

    // -- Shaders and programs ---------------------------------------------------

    pub(crate) fn create_shader(self, stage: GLenum) -> Result<GLuint, GfxError> {
        // SAFETY: see `Gl`. `glCreateShader` takes a value.
        let shader = unsafe { (self.fns.create_shader)(stage) };
        self.after("glCreateShader")?;
        self.named(shader, "glCreateShader gave no name")
    }

    /// Gives a shader its source, with its length, so it needs no NUL.
    pub(crate) fn shader_source(self, shader: GLuint, source: &str) -> Result<(), GfxError> {
        let length = GLint::try_from(source.len())
            .map_err(|_| GfxError::InvalidArgument("a shader source"))?;
        let string = source.as_ptr().cast::<GLchar>();
        // SAFETY: see `Gl`. GL reads one string, of `length` bytes, through
        // the two live pointers, and copies it before it returns.
        unsafe { (self.fns.shader_source)(shader, 1, &raw const string, &raw const length) };
        self.after("glShaderSource")
    }

    pub(crate) fn compile_shader(self, shader: GLuint) -> Result<(), GfxError> {
        // SAFETY: see `Gl`. `glCompileShader` takes a value.
        unsafe { (self.fns.compile_shader)(shader) };
        self.after("glCompileShader")
    }

    /// Whether the last compile of `shader` worked.
    pub(crate) fn shader_compiled(self, shader: GLuint) -> Result<bool, GfxError> {
        Ok(self.shader_parameter(shader, GL_COMPILE_STATUS)? != 0)
    }

    /// One of the single-integer parameters of a shader.
    fn shader_parameter(self, shader: GLuint, pname: GLenum) -> Result<GLint, GfxError> {
        let mut value = 0;
        // SAFETY: see `Gl`. `GL_COMPILE_STATUS` and `GL_INFO_LOG_LENGTH`
        // write one integer, to a live `GLint`.
        unsafe { (self.fns.get_shaderiv)(shader, pname, &raw mut value) };
        self.after("glGetShaderiv")?;
        Ok(value)
    }

    /// What the compiler said about `shader`.
    pub(crate) fn shader_info_log(self, shader: GLuint) -> Result<String, GfxError> {
        let length = self.shader_parameter(shader, GL_INFO_LOG_LENGTH)?;
        let mut log = vec![0u8; usize::try_from(length).unwrap_or(0)];
        let mut written: GLsizei = 0;
        let room = GLsizei::try_from(log.len()).unwrap_or(0);
        // SAFETY: see `Gl`. GL writes at most `room` bytes to `log`, which
        // has them, and the length it wrote to a live `GLsizei`.
        unsafe {
            (self.fns.get_shader_info_log)(
                shader,
                room,
                &raw mut written,
                log.as_mut_ptr().cast::<GLchar>(),
            )
        };
        self.after("glGetShaderInfoLog")?;
        log.truncate(usize::try_from(written).unwrap_or(0));
        Ok(trim_log(&log))
    }

    pub(crate) fn delete_shader(self, shader: GLuint) -> Result<(), GfxError> {
        // SAFETY: see `Gl`. `glDeleteShader` takes a value.
        unsafe { (self.fns.delete_shader)(shader) };
        self.after("glDeleteShader")
    }

    pub(crate) fn create_program(self) -> Result<GLuint, GfxError> {
        // SAFETY: see `Gl`. `glCreateProgram` takes nothing.
        let program = unsafe { (self.fns.create_program)() };
        self.after("glCreateProgram")?;
        self.named(program, "glCreateProgram gave no name")
    }

    pub(crate) fn attach_shader(self, program: GLuint, shader: GLuint) -> Result<(), GfxError> {
        // SAFETY: see `Gl`. `glAttachShader` takes values.
        unsafe { (self.fns.attach_shader)(program, shader) };
        self.after("glAttachShader")
    }

    pub(crate) fn link_program(self, program: GLuint) -> Result<(), GfxError> {
        // SAFETY: see `Gl`. `glLinkProgram` takes a value.
        unsafe { (self.fns.link_program)(program) };
        self.after("glLinkProgram")
    }

    /// Whether the last link of `program` worked.
    pub(crate) fn program_linked(self, program: GLuint) -> Result<bool, GfxError> {
        Ok(self.program_parameter(program, GL_LINK_STATUS)? != 0)
    }

    /// How many uniform blocks the linked `program` uses.
    pub(crate) fn active_uniform_blocks(self, program: GLuint) -> Result<GLint, GfxError> {
        self.program_parameter(program, GL_ACTIVE_UNIFORM_BLOCKS)
    }

    /// One of the single-integer parameters of a program.
    fn program_parameter(self, program: GLuint, pname: GLenum) -> Result<GLint, GfxError> {
        let mut value = 0;
        // SAFETY: see `Gl`. `GL_LINK_STATUS`, `GL_INFO_LOG_LENGTH` and
        // `GL_ACTIVE_UNIFORM_BLOCKS` write one integer, to a live `GLint`.
        unsafe { (self.fns.get_programiv)(program, pname, &raw mut value) };
        self.after("glGetProgramiv")?;
        Ok(value)
    }

    /// What the linker said about `program`.
    pub(crate) fn program_info_log(self, program: GLuint) -> Result<String, GfxError> {
        let length = self.program_parameter(program, GL_INFO_LOG_LENGTH)?;
        let mut log = vec![0u8; usize::try_from(length).unwrap_or(0)];
        let mut written: GLsizei = 0;
        let room = GLsizei::try_from(log.len()).unwrap_or(0);
        // SAFETY: see `Gl`. GL writes at most `room` bytes to `log`, which
        // has them, and the length it wrote to a live `GLsizei`.
        unsafe {
            (self.fns.get_program_info_log)(
                program,
                room,
                &raw mut written,
                log.as_mut_ptr().cast::<GLchar>(),
            )
        };
        self.after("glGetProgramInfoLog")?;
        log.truncate(usize::try_from(written).unwrap_or(0));
        Ok(trim_log(&log))
    }

    pub(crate) fn delete_program(self, program: GLuint) -> Result<(), GfxError> {
        // SAFETY: see `Gl`. `glDeleteProgram` takes a value.
        unsafe { (self.fns.delete_program)(program) };
        self.after("glDeleteProgram")
    }

    pub(crate) fn use_program(self, program: GLuint) -> Result<(), GfxError> {
        // SAFETY: see `Gl`. `glUseProgram` takes a value.
        unsafe { (self.fns.use_program)(program) };
        self.after("glUseProgram")
    }

    /// The location of the uniform `name` of `program`, or -1 if it uses
    /// none of that name.
    pub(crate) fn uniform_location(self, program: GLuint, name: &CStr) -> Result<GLint, GfxError> {
        // SAFETY: see `Gl`. The name is a NUL-terminated string.
        let location = unsafe { (self.fns.get_uniform_location)(program, name.as_ptr()) };
        self.after("glGetUniformLocation")?;
        Ok(location)
    }

    /// Sets an integer uniform of the program in use. -1 does nothing.
    pub(crate) fn uniform_1i(self, location: GLint, value: GLint) -> Result<(), GfxError> {
        // SAFETY: see `Gl`. `glUniform1i` takes values.
        unsafe { (self.fns.uniform_1i)(location, value) };
        self.after("glUniform1i")
    }

    /// The index of the uniform block `name` of `program`, or
    /// `GL_INVALID_INDEX`.
    pub(crate) fn uniform_block_index(
        self,
        program: GLuint,
        name: &CStr,
    ) -> Result<GLuint, GfxError> {
        // SAFETY: see `Gl`. The name is a NUL-terminated string.
        let index = unsafe { (self.fns.get_uniform_block_index)(program, name.as_ptr()) };
        self.after("glGetUniformBlockIndex")?;
        Ok(index)
    }

    /// The size, in bytes, of uniform block `index` of `program`.
    pub(crate) fn uniform_block_size(
        self,
        program: GLuint,
        index: GLuint,
    ) -> Result<GLint, GfxError> {
        let mut value = 0;
        // SAFETY: see `Gl`. `GL_UNIFORM_BLOCK_DATA_SIZE` writes one integer,
        // to a live `GLint`.
        unsafe {
            (self.fns.get_active_uniform_blockiv)(
                program,
                index,
                GL_UNIFORM_BLOCK_DATA_SIZE,
                &raw mut value,
            )
        };
        self.after("glGetActiveUniformBlockiv")?;
        Ok(value)
    }

    pub(crate) fn uniform_block_binding(
        self,
        program: GLuint,
        index: GLuint,
        binding: u32,
    ) -> Result<(), GfxError> {
        // SAFETY: see `Gl`. `glUniformBlockBinding` takes values.
        unsafe { (self.fns.uniform_block_binding)(program, index, binding) };
        self.after("glUniformBlockBinding")
    }

    // -- Framebuffers -----------------------------------------------------------

    pub(crate) fn gen_framebuffer(self) -> Result<GLuint, GfxError> {
        let mut name = 0;
        // SAFETY: see `Gl`. One name goes to a live `GLuint`.
        unsafe { (self.fns.gen_framebuffers)(1, &raw mut name) };
        self.after("glGenFramebuffers")?;
        self.named(name, "glGenFramebuffers gave no name")
    }

    pub(crate) fn delete_framebuffer(self, name: GLuint) -> Result<(), GfxError> {
        // SAFETY: see `Gl`. One name is read from a live `GLuint`.
        unsafe { (self.fns.delete_framebuffers)(1, &raw const name) };
        self.after("glDeleteFramebuffers")
    }

    pub(crate) fn bind_framebuffer(self, name: GLuint) -> Result<(), GfxError> {
        // SAFETY: see `Gl`. `glBindFramebuffer` takes values.
        unsafe { (self.fns.bind_framebuffer)(GL_FRAMEBUFFER, name) };
        self.after("glBindFramebuffer")
    }

    /// Attaches level 0 of a 2D texture as the color of the bound framebuffer.
    pub(crate) fn framebuffer_texture(self, texture: GLuint) -> Result<(), GfxError> {
        // SAFETY: see `Gl`. `glFramebufferTexture2D` takes values.
        unsafe {
            (self.fns.framebuffer_texture_2d)(
                GL_FRAMEBUFFER,
                GL_COLOR_ATTACHMENT0,
                GL_TEXTURE_2D,
                texture,
                0,
            )
        };
        self.after("glFramebufferTexture2D")
    }

    pub(crate) fn framebuffer_status(self) -> Result<GLenum, GfxError> {
        // SAFETY: see `Gl`. `glCheckFramebufferStatus` takes a value.
        let status = unsafe { (self.fns.check_framebuffer_status)(GL_FRAMEBUFFER) };
        self.after("glCheckFramebufferStatus")?;
        Ok(status)
    }

    /// Reads `width` by `height` RGBA8 pixels of the bound framebuffer, from
    /// its bottom row, into `out`.
    pub(crate) fn read_pixels(
        self,
        [width, height]: [u32; 2],
        out: &mut [u8],
    ) -> Result<(), GfxError> {
        let expected = image_bytes(width, height, 4)?;
        if out.len() != expected {
            return Err(GfxError::DataSize {
                expected,
                found: out.len(),
            });
        }
        let (w, h) = (sizei(width)?, sizei(height)?);
        // SAFETY: see `Gl`. Rows of RGBA8 are `4 * width` bytes, which the
        // pack alignment (4, never changed, with no pack buffer bound) adds
        // nothing to, so GL writes `4 * width * height` bytes, which `out`
        // has.
        unsafe {
            (self.fns.read_pixels)(
                0,
                0,
                w,
                h,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                out.as_mut_ptr().cast(),
            )
        };
        self.after("glReadPixels")
    }

    // -- Drawing ----------------------------------------------------------------

    /// Draws `count` vertices from `first`, `instances` times.
    ///
    /// # Safety
    ///
    /// Every attribute that the bound vertex array enables reads, for those
    /// vertices and instances, only inside the buffer it points into, and
    /// every uniform block of the program in use has a range of at least its
    /// size bound.
    pub(crate) unsafe fn draw_arrays_instanced(
        self,
        mode: GLenum,
        first: GLint,
        count: GLsizei,
        instances: GLsizei,
    ) -> Result<(), GfxError> {
        // SAFETY: see `Gl`. The caller promises that what the draw reads is
        // in bounds.
        unsafe { (self.fns.draw_arrays_instanced)(mode, first, count, instances) };
        self.after("glDrawArraysInstanced")
    }

    /// Draws `count` indices of type `index_type`, from byte `offset` of the
    /// element buffer of the bound vertex array, each plus `base_vertex`,
    /// `instances` times.
    ///
    /// # Safety
    ///
    /// The bound vertex array has an element buffer, which holds the
    /// indices read. For each index plus `base_vertex`, and each instance,
    /// every attribute that the vertex array enables reads only inside its
    /// buffer. Every uniform block of the program in use has a range of at
    /// least its size bound.
    pub(crate) unsafe fn draw_elements_instanced_base_vertex(
        self,
        mode: GLenum,
        count: GLsizei,
        index_type: GLenum,
        offset: usize,
        instances: GLsizei,
        base_vertex: GLint,
    ) -> Result<(), GfxError> {
        // An offset into the element buffer, which GL does not follow as a
        // pointer.
        let offset = ptr::without_provenance::<c_void>(offset);
        // SAFETY: see `Gl`. The caller promises that what the draw reads is
        // in bounds.
        unsafe {
            (self.fns.draw_elements_instanced_base_vertex)(
                mode,
                count,
                index_type,
                offset,
                instances,
                base_vertex,
            )
        };
        self.after("glDrawElementsInstancedBaseVertex")
    }
}

/// The bytes of an image of tightly packed rows.
fn image_bytes(width: u32, height: u32, bytes_per_pixel: usize) -> Result<usize, GfxError> {
    usize::try_from(u64::from(width) * u64::from(height))
        .ok()
        .and_then(|pixels| pixels.checked_mul(bytes_per_pixel))
        .ok_or(GfxError::InvalidArgument("an image size"))
}

/// A size, as a `GLsizei`.
fn sizei(value: u32) -> Result<GLsizei, GfxError> {
    GLsizei::try_from(value).map_err(|_| GfxError::InvalidArgument("a size"))
}

/// A coordinate or count, as a `GLint`.
fn int(value: u32) -> Result<GLint, GfxError> {
    GLint::try_from(value).map_err(|_| GfxError::InvalidArgument("a coordinate"))
}
