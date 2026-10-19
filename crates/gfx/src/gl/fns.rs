//! The OpenGL functions the backend calls, looked up for one context.

use core::ffi::{CStr, c_void};

use nadir_platform::GlContext;
use nadir_sys::gl::{
    PFNGLACTIVETEXTUREPROC, PFNGLATTACHSHADERPROC, PFNGLBINDBUFFERPROC, PFNGLBINDBUFFERRANGEPROC,
    PFNGLBINDFRAMEBUFFERPROC, PFNGLBINDSAMPLERPROC, PFNGLBINDTEXTUREPROC, PFNGLBINDVERTEXARRAYPROC,
    PFNGLBLENDFUNCSEPARATEPROC, PFNGLBUFFERDATAPROC, PFNGLBUFFERSUBDATAPROC,
    PFNGLCHECKFRAMEBUFFERSTATUSPROC, PFNGLCLEARCOLORPROC, PFNGLCLEARPROC, PFNGLCOMPILESHADERPROC,
    PFNGLCREATEPROGRAMPROC, PFNGLCREATESHADERPROC, PFNGLDELETEBUFFERSPROC,
    PFNGLDELETEFRAMEBUFFERSPROC, PFNGLDELETEPROGRAMPROC, PFNGLDELETESAMPLERSPROC,
    PFNGLDELETESHADERPROC, PFNGLDELETETEXTURESPROC, PFNGLDELETEVERTEXARRAYSPROC, PFNGLDISABLEPROC,
    PFNGLDRAWARRAYSINSTANCEDPROC, PFNGLDRAWELEMENTSINSTANCEDBASEVERTEXPROC, PFNGLENABLEPROC,
    PFNGLENABLEVERTEXATTRIBARRAYPROC, PFNGLFRAMEBUFFERTEXTURE2DPROC, PFNGLGENBUFFERSPROC,
    PFNGLGENFRAMEBUFFERSPROC, PFNGLGENSAMPLERSPROC, PFNGLGENTEXTURESPROC, PFNGLGENVERTEXARRAYSPROC,
    PFNGLGETACTIVEUNIFORMBLOCKIVPROC, PFNGLGETERRORPROC, PFNGLGETINTEGERVPROC,
    PFNGLGETPROGRAMINFOLOGPROC, PFNGLGETPROGRAMIVPROC, PFNGLGETSHADERINFOLOGPROC,
    PFNGLGETSHADERIVPROC, PFNGLGETSTRINGPROC, PFNGLGETUNIFORMBLOCKINDEXPROC,
    PFNGLGETUNIFORMLOCATIONPROC, PFNGLLINKPROGRAMPROC, PFNGLPIXELSTOREIPROC, PFNGLREADPIXELSPROC,
    PFNGLSAMPLERPARAMETERIPROC, PFNGLSCISSORPROC, PFNGLSHADERSOURCEPROC, PFNGLTEXIMAGE2DPROC,
    PFNGLTEXPARAMETERIPROC, PFNGLTEXSUBIMAGE2DPROC, PFNGLUNIFORM1IPROC,
    PFNGLUNIFORMBLOCKBINDINGPROC, PFNGLUSEPROGRAMPROC, PFNGLVERTEXATTRIBDIVISORPROC,
    PFNGLVERTEXATTRIBPOINTERPROC, PFNGLVIEWPORTPROC,
};

use crate::GfxError;

// A function pointer is converted from the address that the context gives,
// so the two must be the same size.
const _: () = assert!(size_of::<PFNGLCLEARPROC>() == size_of::<*const c_void>());

/// Whether `pfn` is the name of the header's type for the function `name`:
/// `PFNGLCLEARPROC` for `glClear`.
const fn pfn_matches(name: &str, pfn: &str) -> bool {
    let (name, pfn) = (name.as_bytes(), pfn.as_bytes());
    let (prefix, suffix) = (b"PFN", b"PROC");
    if pfn.len() != prefix.len() + name.len() + suffix.len() {
        return false;
    }
    let mut i = 0;
    while i < prefix.len() {
        if pfn[i] != prefix[i] {
            return false;
        }
        i += 1;
    }
    let mut i = 0;
    while i < name.len() {
        if pfn[prefix.len() + i] != name[i].to_ascii_uppercase() {
            return false;
        }
        i += 1;
    }
    let mut i = 0;
    while i < suffix.len() {
        if pfn[prefix.len() + name.len() + i] != suffix[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// Declares [`GlFns`]: one field for each function, of the type that
/// `nadir_sys::gl` declares for it, and `load`, which looks every one up.
///
/// The one `unsafe` operation here is the conversion of an address to its
/// function type. Keep it the only one: clippy does not count the unsafe
/// operations of a macro's expansion.
macro_rules! gl_fns {
    ($($field:ident: $pfn:ident = $name:literal;)+) => {
        /// The OpenGL functions the backend calls, looked up for one
        /// context. Calling one needs that context current: see
        /// [`Gl`](super::calls::Gl).
        pub(crate) struct GlFns {
            $(pub(crate) $field: $pfn,)+
        }

        // A function whose name and type do not go together fails here.
        $(const _: () = assert!(pfn_matches($name, stringify!($pfn)));)+

        impl GlFns {
            /// Looks every function up, before calling any, in the order
            /// they are declared. A name the context does not give is
            /// [`GfxError::MissingFunction`].
            pub(crate) fn load(context: &dyn GlContext) -> Result<Self, GfxError> {
                Ok(Self {
                    $($field: {
                        const NAME: &CStr = match CStr::from_bytes_with_nul(
                            concat!($name, "\0").as_bytes(),
                        ) {
                            Ok(name) => name,
                            Err(_) => panic!("a GL function name has a NUL in it"),
                        };
                        let address = context.get_proc_address(NAME);
                        if address.is_null() {
                            return Err(GfxError::MissingFunction { name: $name });
                        }
                        // SAFETY: the address is not null, so `GlContext`
                        // promises that it is this function, callable through
                        // the type that `nadir_sys::gl` declares for it,
                        // which the assertion above pairs with the name. It
                        // is only converted here.
                        unsafe { core::mem::transmute::<*const c_void, $pfn>(address) }
                    },)+
                })
            }
        }
    };
}

gl_fns! {
    get_error: PFNGLGETERRORPROC = "glGetError";
    get_string: PFNGLGETSTRINGPROC = "glGetString";
    get_integerv: PFNGLGETINTEGERVPROC = "glGetIntegerv";
    enable: PFNGLENABLEPROC = "glEnable";
    disable: PFNGLDISABLEPROC = "glDisable";
    viewport: PFNGLVIEWPORTPROC = "glViewport";
    scissor: PFNGLSCISSORPROC = "glScissor";
    clear_color: PFNGLCLEARCOLORPROC = "glClearColor";
    clear: PFNGLCLEARPROC = "glClear";
    blend_func_separate: PFNGLBLENDFUNCSEPARATEPROC = "glBlendFuncSeparate";
    pixel_storei: PFNGLPIXELSTOREIPROC = "glPixelStorei";
    gen_textures: PFNGLGENTEXTURESPROC = "glGenTextures";
    delete_textures: PFNGLDELETETEXTURESPROC = "glDeleteTextures";
    active_texture: PFNGLACTIVETEXTUREPROC = "glActiveTexture";
    bind_texture: PFNGLBINDTEXTUREPROC = "glBindTexture";
    tex_image_2d: PFNGLTEXIMAGE2DPROC = "glTexImage2D";
    tex_sub_image_2d: PFNGLTEXSUBIMAGE2DPROC = "glTexSubImage2D";
    tex_parameteri: PFNGLTEXPARAMETERIPROC = "glTexParameteri";
    gen_samplers: PFNGLGENSAMPLERSPROC = "glGenSamplers";
    delete_samplers: PFNGLDELETESAMPLERSPROC = "glDeleteSamplers";
    bind_sampler: PFNGLBINDSAMPLERPROC = "glBindSampler";
    sampler_parameteri: PFNGLSAMPLERPARAMETERIPROC = "glSamplerParameteri";
    gen_buffers: PFNGLGENBUFFERSPROC = "glGenBuffers";
    delete_buffers: PFNGLDELETEBUFFERSPROC = "glDeleteBuffers";
    bind_buffer: PFNGLBINDBUFFERPROC = "glBindBuffer";
    buffer_data: PFNGLBUFFERDATAPROC = "glBufferData";
    buffer_sub_data: PFNGLBUFFERSUBDATAPROC = "glBufferSubData";
    bind_buffer_range: PFNGLBINDBUFFERRANGEPROC = "glBindBufferRange";
    gen_vertex_arrays: PFNGLGENVERTEXARRAYSPROC = "glGenVertexArrays";
    delete_vertex_arrays: PFNGLDELETEVERTEXARRAYSPROC = "glDeleteVertexArrays";
    bind_vertex_array: PFNGLBINDVERTEXARRAYPROC = "glBindVertexArray";
    enable_vertex_attrib_array: PFNGLENABLEVERTEXATTRIBARRAYPROC = "glEnableVertexAttribArray";
    vertex_attrib_pointer: PFNGLVERTEXATTRIBPOINTERPROC = "glVertexAttribPointer";
    vertex_attrib_divisor: PFNGLVERTEXATTRIBDIVISORPROC = "glVertexAttribDivisor";
    create_shader: PFNGLCREATESHADERPROC = "glCreateShader";
    shader_source: PFNGLSHADERSOURCEPROC = "glShaderSource";
    compile_shader: PFNGLCOMPILESHADERPROC = "glCompileShader";
    get_shaderiv: PFNGLGETSHADERIVPROC = "glGetShaderiv";
    get_shader_info_log: PFNGLGETSHADERINFOLOGPROC = "glGetShaderInfoLog";
    delete_shader: PFNGLDELETESHADERPROC = "glDeleteShader";
    create_program: PFNGLCREATEPROGRAMPROC = "glCreateProgram";
    attach_shader: PFNGLATTACHSHADERPROC = "glAttachShader";
    link_program: PFNGLLINKPROGRAMPROC = "glLinkProgram";
    get_programiv: PFNGLGETPROGRAMIVPROC = "glGetProgramiv";
    get_program_info_log: PFNGLGETPROGRAMINFOLOGPROC = "glGetProgramInfoLog";
    delete_program: PFNGLDELETEPROGRAMPROC = "glDeleteProgram";
    use_program: PFNGLUSEPROGRAMPROC = "glUseProgram";
    get_uniform_location: PFNGLGETUNIFORMLOCATIONPROC = "glGetUniformLocation";
    uniform_1i: PFNGLUNIFORM1IPROC = "glUniform1i";
    get_uniform_block_index: PFNGLGETUNIFORMBLOCKINDEXPROC = "glGetUniformBlockIndex";
    uniform_block_binding: PFNGLUNIFORMBLOCKBINDINGPROC = "glUniformBlockBinding";
    get_active_uniform_blockiv: PFNGLGETACTIVEUNIFORMBLOCKIVPROC = "glGetActiveUniformBlockiv";
    gen_framebuffers: PFNGLGENFRAMEBUFFERSPROC = "glGenFramebuffers";
    delete_framebuffers: PFNGLDELETEFRAMEBUFFERSPROC = "glDeleteFramebuffers";
    bind_framebuffer: PFNGLBINDFRAMEBUFFERPROC = "glBindFramebuffer";
    framebuffer_texture_2d: PFNGLFRAMEBUFFERTEXTURE2DPROC = "glFramebufferTexture2D";
    check_framebuffer_status: PFNGLCHECKFRAMEBUFFERSTATUSPROC = "glCheckFramebufferStatus";
    read_pixels: PFNGLREADPIXELSPROC = "glReadPixels";
    draw_arrays_instanced: PFNGLDRAWARRAYSINSTANCEDPROC = "glDrawArraysInstanced";
    draw_elements_instanced_base_vertex: PFNGLDRAWELEMENTSINSTANCEDBASEVERTEXPROC =
        "glDrawElementsInstancedBaseVertex";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_matches_only_the_type_of_its_function() {
        assert!(pfn_matches("glClear", "PFNGLCLEARPROC"));
        assert!(pfn_matches("glTexImage2D", "PFNGLTEXIMAGE2DPROC"));
        assert!(!pfn_matches("glClear", "PFNGLCLEARCOLORPROC"));
        assert!(!pfn_matches("glClearColor", "PFNGLCLEARPROC"));
        assert!(!pfn_matches("glClear", "XFNGLCLEARPROC"));
        assert!(!pfn_matches("glClear", "PFNGLCLEARPROX"));
    }
}
