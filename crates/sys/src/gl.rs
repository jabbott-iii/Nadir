//! OpenGL 3.3, core profile: its types, and the constants and function
//! types that the engine calls. Shared by every system.
//!
//! Sources: `<GL/glcorearb.h>`, the Khronos header for core-profile
//! implementations, as libglvnd 1.7.0 installs it, and `<KHR/khrplatform.h>`
//! for the integer types. Each item names its header and quotes the
//! declaration there; a function type also says which GL version brought
//! it.
//!
//! Only what is called is declared: by `nadir-gfx`, and by
//! `nadir-platform`'s check of a new context (`glGetString`, and
//! `glGetIntegerv` with `GL_MAJOR_VERSION`, `GL_MINOR_VERSION` and
//! `GL_CONTEXT_PROFILE_MASK`).
//!
//! # Functions
//!
//! GL functions are not linked. Each one is looked up while the program
//! runs, through the loader of the current context (`eglGetProcAddress`,
//! `wglGetProcAddress`, or a symbol of the OpenGL framework), and cast to
//! its type here. The types keep the names of the header's typedefs:
//! `glClear` is called through a [`PFNGLCLEARPROC`].
//!
//! Two loaders have a second source. On Linux, a function that
//! `eglGetProcAddress` does not return is taken from `libOpenGL.so.0`, the
//! dispatch library of libglvnd, which calls into the context current on
//! the thread. On Windows, `wglGetProcAddress` finds only the functions of
//! versions after GL 1.1; those of GL 1.0 and 1.1 are exports of
//! `opengl32.dll` itself.
//!
//! The calling convention is `extern "system"`. The header takes `APIENTRY`
//! from `<windows.h>` on Windows, where it is `__stdcall`, which only 32-bit
//! x86 honors, and leaves it empty on every other system. `"system"` is
//! stdcall on 32-bit x86 Windows and the C convention everywhere else, so it
//! matches on every target.
//!
//! # Constants
//!
//! A constant keeps its header's name and value. The header gives only
//! `GL_INVALID_INDEX` a type: its `u` suffix makes it an `unsigned int`,
//! here a `GLuint`. Each of the others has the type of the parameter it is
//! passed to, or of the value it is compared with: an internal format is
//! passed as a `GLint` to `glTexImage2D`, and a filter as a `GLint` to
//! `glSamplerParameteri`.
//!
//! # Calling GL
//!
//! What every function here requires of its caller:
//!
//! - A context is current on this thread, of GL 3.3 or later, which has
//!   every function declared here (`nadir-gfx` checks the version before
//!   it calls anything else). The function pointer came from one of the
//!   loaders above, for that context or one equivalent to it.
//! - A pointer argument is valid for the bytes the call reads or writes
//!   through it, which its other arguments describe. Where none gives a
//!   length (the names of `glGetUniformLocation` and
//!   `glGetUniformBlockIndex`, and the strings of `glShaderSource` with a
//!   null `length`), the string ends with a NUL.
//! - Pixels in memory are `height` rows of `width` pixels. Rows that a
//!   texture is read from are padded to `GL_UNPACK_ALIGNMENT`, and rows that
//!   `glReadPixels` writes to the pack alignment, which starts at 4 and
//!   which the engine leaves alone; the other pixel-store values (row
//!   length, skips) stay 0, as they start. With no buffer bound to the
//!   pixel-unpack or pixel-pack target (the engine binds neither), a pixel
//!   pointer is an address, not an offset.
//! - An offset argument named `pointer` or `indices` is a byte offset into
//!   the bound buffer, cast to a pointer, and a multiple of the size of the
//!   value it points at.
//! - A draw reads only inside buffers. Each enabled attribute was given its
//!   pointer while a buffer was bound to `GL_ARRAY_BUFFER`; every vertex and
//!   instance that the draw uses lies inside that buffer; each index, plus
//!   `basevertex`, names such a vertex; and the indices lie inside the
//!   element buffer of the bound vertex array. GL does not always detect a
//!   draw that reads outside, and one it does not detect is undefined, up to
//!   the end of the program (the GL 3.3 core specification, "Vertex Arrays"
//!   and "Buffer Objects").
//!
//! An object name that is stale, or of the wrong kind, is not a memory
//! error. GL sets an error flag, which `glGetError` returns; or it ignores
//! the name, as the `glDelete*` functions do with names that are no
//! objects; or it acts on whatever object of that kind has the number now,
//! because the `glGen*` functions hand deleted names out again, and each
//! kind of object has numbers of its own. Keeping names current is the
//! caller's work: `nadir-gfx` does it with generational ids.

// The parameters keep the names their header gives them (`infoLog`,
// `sfactorRGB`), so that each one can be compared with it.
#![allow(non_snake_case)]

use core::ffi::{c_char, c_void};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// An enum value. `GL/glcorearb.h`: `typedef unsigned int GLenum;`
pub type GLenum = u32;

/// A truth value: 1 or [`GL_FALSE`]. `GL/glcorearb.h`: `typedef unsigned char
/// GLboolean;`
pub type GLboolean = u8;

/// A set of bits. `GL/glcorearb.h`: `typedef unsigned int GLbitfield;`
pub type GLbitfield = u32;

/// A signed integer. `GL/glcorearb.h`: `typedef int GLint;`
pub type GLint = i32;

/// An unsigned integer, and the name of an object. `GL/glcorearb.h`:
/// `typedef unsigned int GLuint;`
pub type GLuint = u32;

/// A count or a size. `GL/glcorearb.h`: `typedef int GLsizei;`
pub type GLsizei = i32;

/// A float. `GL/glcorearb.h`: `typedef khronos_float_t GLfloat;`, and
/// `KHR/khrplatform.h`: `typedef float khronos_float_t;`
pub type GLfloat = f32;

/// A character of a string, such as a shader's source. `GL/glcorearb.h`
/// (GL 2.0): `typedef char GLchar;`
pub type GLchar = c_char;

/// A byte, and a character of the strings `glGetString` returns.
/// `GL/glcorearb.h`: `typedef khronos_uint8_t GLubyte;`, and
/// `KHR/khrplatform.h`: `typedef unsigned char khronos_uint8_t;`
pub type GLubyte = u8;

/// The size of a buffer, in bytes. `GL/glcorearb.h` (GL 1.5): `typedef
/// khronos_ssize_t GLsizeiptr;`, and `KHR/khrplatform.h`: `typedef signed
/// long int khronos_ssize_t;` (on 64-bit Unix; `long long` on 64-bit
/// Windows, the size of a pointer on both)
pub type GLsizeiptr = isize;

/// An offset into a buffer, in bytes. `GL/glcorearb.h` (GL 1.5): `typedef
/// khronos_intptr_t GLintptr;`, and `KHR/khrplatform.h`: `typedef signed
/// long int khronos_intptr_t;` (on 64-bit Unix; `long long` on 64-bit
/// Windows, the size of a pointer on both)
pub type GLintptr = isize;

// ---------------------------------------------------------------------------
// Constants: booleans and errors
// ---------------------------------------------------------------------------

/// False: the `normalized` argument of `glVertexAttribPointer`, and what a
/// status parameter reads after a failure.
/// `GL/glcorearb.h` (GL 1.0): `#define GL_FALSE 0`
pub const GL_FALSE: GLboolean = 0;

/// What `glGetError` returns when no error flag is set.
/// `GL/glcorearb.h` (GL 1.0): `#define GL_NO_ERROR 0`
pub const GL_NO_ERROR: GLenum = 0;

/// An error: an enum argument is not one that the call accepts.
/// `GL/glcorearb.h` (GL 1.0): `#define GL_INVALID_ENUM 0x0500`
pub const GL_INVALID_ENUM: GLenum = 0x0500;

/// An error: a numeric argument is out of range.
/// `GL/glcorearb.h` (GL 1.0): `#define GL_INVALID_VALUE 0x0501`
pub const GL_INVALID_VALUE: GLenum = 0x0501;

/// An error: the call is not allowed in the current state.
/// `GL/glcorearb.h` (GL 1.0): `#define GL_INVALID_OPERATION 0x0502`
pub const GL_INVALID_OPERATION: GLenum = 0x0502;

/// An error: there is not enough memory to do the call. The state of GL is
/// undefined after it.
/// `GL/glcorearb.h` (GL 1.0): `#define GL_OUT_OF_MEMORY 0x0505`
pub const GL_OUT_OF_MEMORY: GLenum = 0x0505;

/// An error: the bound framebuffer is not complete, so it cannot be drawn to or
/// read from.
/// `GL/glcorearb.h` (GL 3.0): `#define GL_INVALID_FRAMEBUFFER_OPERATION 0x0506`
pub const GL_INVALID_FRAMEBUFFER_OPERATION: GLenum = 0x0506;

/// What `glGetUniformBlockIndex` returns for a name that is no active uniform
/// block of the program.
/// `GL/glcorearb.h` (GL 3.1): `#define GL_INVALID_INDEX 0xFFFFFFFFu`
pub const GL_INVALID_INDEX: GLuint = 0xFFFF_FFFF;

// ---------------------------------------------------------------------------
// Constants: strings and states
// ---------------------------------------------------------------------------

/// For `glGetString`: the company behind the driver.
/// `GL/glcorearb.h` (GL 1.0): `#define GL_VENDOR 0x1F00`
pub const GL_VENDOR: GLenum = 0x1F00;

/// For `glGetString`: the name of the renderer, often the GPU's.
/// `GL/glcorearb.h` (GL 1.0): `#define GL_RENDERER 0x1F01`
pub const GL_RENDERER: GLenum = 0x1F01;

/// For `glGetString`: the GL version, as `<major>.<minor>` or
/// `<major>.<minor>.<release>`, then, optionally, a space and text of the
/// vendor's.
/// `GL/glcorearb.h` (GL 1.0): `#define GL_VERSION 0x1F02`
pub const GL_VERSION: GLenum = 0x1F02;

/// For `glGetIntegerv`: the major version of the context.
/// `GL/glcorearb.h` (GL 3.0): `#define GL_MAJOR_VERSION 0x821B`
pub const GL_MAJOR_VERSION: GLenum = 0x821B;

/// For `glGetIntegerv`: the minor version of the context.
/// `GL/glcorearb.h` (GL 3.0): `#define GL_MINOR_VERSION 0x821C`
pub const GL_MINOR_VERSION: GLenum = 0x821C;

/// For `glGetIntegerv`: the profile of the context, as
/// `GL_CONTEXT_*_PROFILE_BIT`s.
/// `GL/glcorearb.h` (GL 3.2): `#define GL_CONTEXT_PROFILE_MASK 0x9126`
pub const GL_CONTEXT_PROFILE_MASK: GLenum = 0x9126;

/// A bit of `GL_CONTEXT_PROFILE_MASK`: the context has the core profile.
/// `GL/glcorearb.h` (GL 3.2): `#define GL_CONTEXT_CORE_PROFILE_BIT 0x00000001`
pub const GL_CONTEXT_CORE_PROFILE_BIT: GLint = 0x0000_0001;

/// For `glGetIntegerv`: the largest width or height of a 2D texture, in pixels.
/// `GL/glcorearb.h` (GL 1.0): `#define GL_MAX_TEXTURE_SIZE 0x0D33`
pub const GL_MAX_TEXTURE_SIZE: GLenum = 0x0D33;

/// For `glGetIntegerv`: what the `offset` of `glBindBufferRange` must be a
/// multiple of, for `GL_UNIFORM_BUFFER`.
/// `GL/glcorearb.h` (GL 3.1): `#define GL_UNIFORM_BUFFER_OFFSET_ALIGNMENT
/// 0x8A34`
pub const GL_UNIFORM_BUFFER_OFFSET_ALIGNMENT: GLenum = 0x8A34;

// ---------------------------------------------------------------------------
// Constants: capabilities
// ---------------------------------------------------------------------------

/// For `glEnable`: blend each fragment with the pixel under it.
/// `GL/glcorearb.h` (GL 1.0): `#define GL_BLEND 0x0BE2`
pub const GL_BLEND: GLenum = 0x0BE2;

/// For `glEnable`: discard what falls outside the scissor rectangle.
/// `GL/glcorearb.h` (GL 1.0): `#define GL_SCISSOR_TEST 0x0C11`
pub const GL_SCISSOR_TEST: GLenum = 0x0C11;

/// For `glEnable`: convert to sRGB on writing to an sRGB framebuffer, and blend
/// in linear light.
/// `GL/glcorearb.h` (GL 3.0): `#define GL_FRAMEBUFFER_SRGB 0x8DB9`
pub const GL_FRAMEBUFFER_SRGB: GLenum = 0x8DB9;

// ---------------------------------------------------------------------------
// Constants: clearing and blending
// ---------------------------------------------------------------------------

/// For `glClear`: the color buffers.
/// `GL/glcorearb.h` (GL 1.0): `#define GL_COLOR_BUFFER_BIT 0x00004000`
pub const GL_COLOR_BUFFER_BIT: GLbitfield = 0x0000_4000;

/// A blend factor: 0.
/// `GL/glcorearb.h` (GL 1.0): `#define GL_ZERO 0`
pub const GL_ZERO: GLenum = 0;

/// A blend factor: 1.
/// `GL/glcorearb.h` (GL 1.0): `#define GL_ONE 1`
pub const GL_ONE: GLenum = 1;

/// A blend factor: the alpha of the new fragment.
/// `GL/glcorearb.h` (GL 1.0): `#define GL_SRC_ALPHA 0x0302`
pub const GL_SRC_ALPHA: GLenum = 0x0302;

/// A blend factor: 1 minus the alpha of the new fragment.
/// `GL/glcorearb.h` (GL 1.0): `#define GL_ONE_MINUS_SRC_ALPHA 0x0303`
pub const GL_ONE_MINUS_SRC_ALPHA: GLenum = 0x0303;

/// A blend factor: the color already in the framebuffer.
/// `GL/glcorearb.h` (GL 1.0): `#define GL_DST_COLOR 0x0306`
pub const GL_DST_COLOR: GLenum = 0x0306;

// ---------------------------------------------------------------------------
// Constants: pixel data
// ---------------------------------------------------------------------------

/// For `glPixelStorei`: the alignment, in bytes, of each row of pixels that a
/// texture is read from. It starts at 4.
/// `GL/glcorearb.h` (GL 1.0): `#define GL_UNPACK_ALIGNMENT 0x0CF5`
pub const GL_UNPACK_ALIGNMENT: GLenum = 0x0CF5;

/// A data type: `u8`, for pixel channels.
/// `GL/glcorearb.h` (GL 1.0): `#define GL_UNSIGNED_BYTE 0x1401`
pub const GL_UNSIGNED_BYTE: GLenum = 0x1401;

/// A data type: `u16`, for indices.
/// `GL/glcorearb.h` (GL 1.0): `#define GL_UNSIGNED_SHORT 0x1403`
pub const GL_UNSIGNED_SHORT: GLenum = 0x1403;

/// A data type: `u32`, for indices.
/// `GL/glcorearb.h` (GL 1.0): `#define GL_UNSIGNED_INT 0x1405`
pub const GL_UNSIGNED_INT: GLenum = 0x1405;

/// A data type: `f32`, for vertex attributes.
/// `GL/glcorearb.h` (GL 1.0): `#define GL_FLOAT 0x1406`
pub const GL_FLOAT: GLenum = 0x1406;

/// A pixel format in memory: red, green, blue and alpha.
/// `GL/glcorearb.h` (GL 1.0): `#define GL_RGBA 0x1908`
pub const GL_RGBA: GLenum = 0x1908;

/// A pixel format in memory: red alone.
/// `GL/glcorearb.h` (GL 1.0): `#define GL_RED 0x1903`
pub const GL_RED: GLenum = 0x1903;

// ---------------------------------------------------------------------------
// Constants: textures
// ---------------------------------------------------------------------------

/// The texture target of 2D textures.
/// `GL/glcorearb.h` (GL 1.0): `#define GL_TEXTURE_2D 0x0DE1`
pub const GL_TEXTURE_2D: GLenum = 0x0DE1;

/// The first texture unit, for `glActiveTexture`. Unit `n` is `GL_TEXTURE0 +
/// n`.
/// `GL/glcorearb.h` (GL 1.3): `#define GL_TEXTURE0 0x84C0`
pub const GL_TEXTURE0: GLenum = 0x84C0;

/// An internal format: 8 bits per channel, red, green, blue and alpha.
/// `GL/glcorearb.h` (GL 1.1): `#define GL_RGBA8 0x8058`
pub const GL_RGBA8: GLint = 0x8058;

/// An internal format: sRGB-encoded red, green and blue, and linear alpha, 8
/// bits each.
/// `GL/glcorearb.h` (GL 2.1): `#define GL_SRGB8_ALPHA8 0x8C43`
pub const GL_SRGB8_ALPHA8: GLint = 0x8C43;

/// An internal format: 8 bits of red.
/// `GL/glcorearb.h` (GL 3.0): `#define GL_R8 0x8229`
pub const GL_R8: GLint = 0x8229;

/// A texture parameter: the last mipmap level. 0 makes a texture of one level
/// complete.
/// `GL/glcorearb.h` (GL 1.2): `#define GL_TEXTURE_MAX_LEVEL 0x813D`
pub const GL_TEXTURE_MAX_LEVEL: GLenum = 0x813D;

/// A sampler parameter: the filter for a texture drawn smaller than it is.
/// `GL/glcorearb.h` (GL 1.0): `#define GL_TEXTURE_MIN_FILTER 0x2801`
pub const GL_TEXTURE_MIN_FILTER: GLenum = 0x2801;

/// A sampler parameter: the filter for a texture drawn larger than it is.
/// `GL/glcorearb.h` (GL 1.0): `#define GL_TEXTURE_MAG_FILTER 0x2800`
pub const GL_TEXTURE_MAG_FILTER: GLenum = 0x2800;

/// A sampler parameter: what lies left and right of the texture.
/// `GL/glcorearb.h` (GL 1.0): `#define GL_TEXTURE_WRAP_S 0x2802`
pub const GL_TEXTURE_WRAP_S: GLenum = 0x2802;

/// A sampler parameter: what lies above and below the texture.
/// `GL/glcorearb.h` (GL 1.0): `#define GL_TEXTURE_WRAP_T 0x2803`
pub const GL_TEXTURE_WRAP_T: GLenum = 0x2803;

/// A filter: the nearest texel.
/// `GL/glcorearb.h` (GL 1.0): `#define GL_NEAREST 0x2600`
pub const GL_NEAREST: GLint = 0x2600;

/// A filter: a blend of the four nearest texels.
/// `GL/glcorearb.h` (GL 1.0): `#define GL_LINEAR 0x2601`
pub const GL_LINEAR: GLint = 0x2601;

/// A wrap mode: the edge texel goes on.
/// `GL/glcorearb.h` (GL 1.2): `#define GL_CLAMP_TO_EDGE 0x812F`
pub const GL_CLAMP_TO_EDGE: GLint = 0x812F;

/// A wrap mode: the texture repeats.
/// `GL/glcorearb.h` (GL 1.0): `#define GL_REPEAT 0x2901`
pub const GL_REPEAT: GLint = 0x2901;

// ---------------------------------------------------------------------------
// Constants: buffers
// ---------------------------------------------------------------------------

/// A buffer target: vertex and instance data.
/// `GL/glcorearb.h` (GL 1.5): `#define GL_ARRAY_BUFFER 0x8892`
pub const GL_ARRAY_BUFFER: GLenum = 0x8892;

/// A buffer target: indices, for the bound vertex array.
/// `GL/glcorearb.h` (GL 1.5): `#define GL_ELEMENT_ARRAY_BUFFER 0x8893`
pub const GL_ELEMENT_ARRAY_BUFFER: GLenum = 0x8893;

/// A buffer target: uniform blocks.
/// `GL/glcorearb.h` (GL 3.1): `#define GL_UNIFORM_BUFFER 0x8A11`
pub const GL_UNIFORM_BUFFER: GLenum = 0x8A11;

/// A usage hint: written once, drawn from many times.
/// `GL/glcorearb.h` (GL 1.5): `#define GL_STATIC_DRAW 0x88E4`
pub const GL_STATIC_DRAW: GLenum = 0x88E4;

/// A usage hint: written often, drawn from many times.
/// `GL/glcorearb.h` (GL 1.5): `#define GL_DYNAMIC_DRAW 0x88E8`
pub const GL_DYNAMIC_DRAW: GLenum = 0x88E8;

// ---------------------------------------------------------------------------
// Constants: shaders and programs
// ---------------------------------------------------------------------------

/// A shader stage: vertices.
/// `GL/glcorearb.h` (GL 2.0): `#define GL_VERTEX_SHADER 0x8B31`
pub const GL_VERTEX_SHADER: GLenum = 0x8B31;

/// A shader stage: fragments.
/// `GL/glcorearb.h` (GL 2.0): `#define GL_FRAGMENT_SHADER 0x8B30`
pub const GL_FRAGMENT_SHADER: GLenum = 0x8B30;

/// A shader parameter: whether the last compile worked.
/// `GL/glcorearb.h` (GL 2.0): `#define GL_COMPILE_STATUS 0x8B81`
pub const GL_COMPILE_STATUS: GLenum = 0x8B81;

/// A program parameter: whether the last link worked.
/// `GL/glcorearb.h` (GL 2.0): `#define GL_LINK_STATUS 0x8B82`
pub const GL_LINK_STATUS: GLenum = 0x8B82;

/// A shader or program parameter: the length of the info log, NUL included, or 0.
/// `GL/glcorearb.h` (GL 2.0): `#define GL_INFO_LOG_LENGTH 0x8B84`
pub const GL_INFO_LOG_LENGTH: GLenum = 0x8B84;

/// A program parameter: how many uniform blocks the linked program uses.
/// `GL/glcorearb.h` (GL 3.1): `#define GL_ACTIVE_UNIFORM_BLOCKS 0x8A36`
pub const GL_ACTIVE_UNIFORM_BLOCKS: GLenum = 0x8A36;

/// A uniform block parameter: the size of the block's data, in bytes.
/// `GL/glcorearb.h` (GL 3.1): `#define GL_UNIFORM_BLOCK_DATA_SIZE 0x8A40`
pub const GL_UNIFORM_BLOCK_DATA_SIZE: GLenum = 0x8A40;

// ---------------------------------------------------------------------------
// Constants: framebuffers
// ---------------------------------------------------------------------------

/// The framebuffer target for drawing and reading both.
/// `GL/glcorearb.h` (GL 3.0): `#define GL_FRAMEBUFFER 0x8D40`
pub const GL_FRAMEBUFFER: GLenum = 0x8D40;

/// The first color attachment point of a framebuffer.
/// `GL/glcorearb.h` (GL 3.0): `#define GL_COLOR_ATTACHMENT0 0x8CE0`
pub const GL_COLOR_ATTACHMENT0: GLenum = 0x8CE0;

/// The framebuffer can be drawn to.
/// `GL/glcorearb.h` (GL 3.0): `#define GL_FRAMEBUFFER_COMPLETE 0x8CD5`
pub const GL_FRAMEBUFFER_COMPLETE: GLenum = 0x8CD5;

/// An attachment cannot be drawn to, such as a texture of no size or of a
/// format that cannot be rendered to.
/// `GL/glcorearb.h` (GL 3.0): `#define GL_FRAMEBUFFER_INCOMPLETE_ATTACHMENT
/// 0x8CD6`
pub const GL_FRAMEBUFFER_INCOMPLETE_ATTACHMENT: GLenum = 0x8CD6;

/// The driver does not support this combination of attachments.
/// `GL/glcorearb.h` (GL 3.0): `#define GL_FRAMEBUFFER_UNSUPPORTED 0x8CDD`
pub const GL_FRAMEBUFFER_UNSUPPORTED: GLenum = 0x8CDD;

// ---------------------------------------------------------------------------
// Constants: primitives
// ---------------------------------------------------------------------------

/// Each three vertices make a triangle.
/// `GL/glcorearb.h` (GL 1.0): `#define GL_TRIANGLES 0x0004`
pub const GL_TRIANGLES: GLenum = 0x0004;

/// Each vertex after the second makes a triangle with the two before it.
/// `GL/glcorearb.h` (GL 1.0): `#define GL_TRIANGLE_STRIP 0x0005`
pub const GL_TRIANGLE_STRIP: GLenum = 0x0005;

/// Each two vertices make a line.
/// `GL/glcorearb.h` (GL 1.0): `#define GL_LINES 0x0001`
pub const GL_LINES: GLenum = 0x0001;

// ---------------------------------------------------------------------------
// Functions: state and errors
// ---------------------------------------------------------------------------

/// `glGetError`: Returns an error flag that is set, and clears it, or
/// `GL_NO_ERROR` if none is. Several can be set: call it until it returns
/// `GL_NO_ERROR`.
/// `GL/glcorearb.h` (GL 1.0): `typedef GLenum (APIENTRYP PFNGLGETERRORPROC)
/// (void);`
pub type PFNGLGETERRORPROC = unsafe extern "system" fn() -> GLenum;

/// `glGetString`: Returns a string about the context, which ends with a NUL and
/// lives as long as the context: `GL_VENDOR`, `GL_RENDERER` or `GL_VERSION`.
/// Returns null if the call fails, which must be checked before the string is
/// read. The context owns it; do not free it.
/// `GL/glcorearb.h` (GL 1.0): `typedef const GLubyte *(APIENTRYP
/// PFNGLGETSTRINGPROC) (GLenum name);`
pub type PFNGLGETSTRINGPROC = unsafe extern "system" fn(name: GLenum) -> *const GLubyte;

/// `glGetIntegerv`: Writes the value of an integer state, such as
/// `GL_MAX_TEXTURE_SIZE`, to `data`. A state with more values writes them all.
/// `GL/glcorearb.h` (GL 1.0): `typedef void (APIENTRYP PFNGLGETINTEGERVPROC)
/// (GLenum pname, GLint *data);`
pub type PFNGLGETINTEGERVPROC = unsafe extern "system" fn(pname: GLenum, data: *mut GLint);

/// `glEnable`: Turns a capability on: `GL_BLEND`, `GL_SCISSOR_TEST` or
/// `GL_FRAMEBUFFER_SRGB`.
/// `GL/glcorearb.h` (GL 1.0): `typedef void (APIENTRYP PFNGLENABLEPROC) (GLenum
/// cap);`
pub type PFNGLENABLEPROC = unsafe extern "system" fn(cap: GLenum);

/// `glDisable`: Turns a capability off.
/// `GL/glcorearb.h` (GL 1.0): `typedef void (APIENTRYP PFNGLDISABLEPROC)
/// (GLenum cap);`
pub type PFNGLDISABLEPROC = unsafe extern "system" fn(cap: GLenum);

/// `glViewport`: Sets the rectangle of the framebuffer that drawing maps to, in
/// pixels from its bottom-left corner.
/// `GL/glcorearb.h` (GL 1.0): `typedef void (APIENTRYP PFNGLVIEWPORTPROC)
/// (GLint x, GLint y, GLsizei width, GLsizei height);`
pub type PFNGLVIEWPORTPROC =
    unsafe extern "system" fn(x: GLint, y: GLint, width: GLsizei, height: GLsizei);

/// `glScissor`: Sets the rectangle outside which drawing and clearing are
/// discarded while `GL_SCISSOR_TEST` is on, in pixels from the bottom-left
/// corner.
/// `GL/glcorearb.h` (GL 1.0): `typedef void (APIENTRYP PFNGLSCISSORPROC) (GLint
/// x, GLint y, GLsizei width, GLsizei height);`
pub type PFNGLSCISSORPROC =
    unsafe extern "system" fn(x: GLint, y: GLint, width: GLsizei, height: GLsizei);

/// `glClearColor`: Sets the color that `glClear` fills with.
/// `GL/glcorearb.h` (GL 1.0): `typedef void (APIENTRYP PFNGLCLEARCOLORPROC)
/// (GLfloat red, GLfloat green, GLfloat blue, GLfloat alpha);`
pub type PFNGLCLEARCOLORPROC =
    unsafe extern "system" fn(red: GLfloat, green: GLfloat, blue: GLfloat, alpha: GLfloat);

/// `glClear`: Fills the buffers that `mask` names, of the bound draw
/// framebuffer, inside the scissor rectangle if the test is on.
/// `GL/glcorearb.h` (GL 1.0): `typedef void (APIENTRYP PFNGLCLEARPROC)
/// (GLbitfield mask);`
pub type PFNGLCLEARPROC = unsafe extern "system" fn(mask: GLbitfield);

/// `glBlendFuncSeparate`: Sets the blend factors, for the color channels and
/// for alpha apart, that apply while `GL_BLEND` is on.
/// `GL/glcorearb.h` (GL 1.4): `typedef void (APIENTRYP
/// PFNGLBLENDFUNCSEPARATEPROC) (GLenum sfactorRGB, GLenum dfactorRGB, GLenum
/// sfactorAlpha, GLenum dfactorAlpha);`
pub type PFNGLBLENDFUNCSEPARATEPROC = unsafe extern "system" fn(
    sfactorRGB: GLenum,
    dfactorRGB: GLenum,
    sfactorAlpha: GLenum,
    dfactorAlpha: GLenum,
);

/// `glPixelStorei`: Sets how pixel data in memory is laid out, such as the
/// alignment of each row that a texture is read from (`GL_UNPACK_ALIGNMENT`).
/// `GL/glcorearb.h` (GL 1.0): `typedef void (APIENTRYP PFNGLPIXELSTOREIPROC)
/// (GLenum pname, GLint param);`
pub type PFNGLPIXELSTOREIPROC = unsafe extern "system" fn(pname: GLenum, param: GLint);

// ---------------------------------------------------------------------------
// Functions: textures
// ---------------------------------------------------------------------------

/// `glGenTextures`: Writes `n` new texture names to `textures`.
/// `GL/glcorearb.h` (GL 1.1): `typedef void (APIENTRYP PFNGLGENTEXTURESPROC)
/// (GLsizei n, GLuint *textures);`
pub type PFNGLGENTEXTURESPROC = unsafe extern "system" fn(n: GLsizei, textures: *mut GLuint);

/// `glDeleteTextures`: Deletes `n` textures. Zero, and names that are no
/// textures, are ignored.
/// `GL/glcorearb.h` (GL 1.1): `typedef void (APIENTRYP PFNGLDELETETEXTURESPROC)
/// (GLsizei n, const GLuint *textures);`
pub type PFNGLDELETETEXTURESPROC = unsafe extern "system" fn(n: GLsizei, textures: *const GLuint);

/// `glActiveTexture`: Selects the texture unit that `glBindTexture` binds to:
/// `GL_TEXTURE0` plus the unit's number.
/// `GL/glcorearb.h` (GL 1.3): `typedef void (APIENTRYP PFNGLACTIVETEXTUREPROC)
/// (GLenum texture);`
pub type PFNGLACTIVETEXTUREPROC = unsafe extern "system" fn(texture: GLenum);

/// `glBindTexture`: Binds a texture to a target of the active texture unit.
/// `GL/glcorearb.h` (GL 1.1): `typedef void (APIENTRYP PFNGLBINDTEXTUREPROC)
/// (GLenum target, GLuint texture);`
pub type PFNGLBINDTEXTUREPROC = unsafe extern "system" fn(target: GLenum, texture: GLuint);

/// `glTexImage2D`: Gives a level of the bound 2D texture its size and format,
/// and its pixels from `pixels` unless that is null.
/// `GL/glcorearb.h` (GL 1.0): `typedef void (APIENTRYP PFNGLTEXIMAGE2DPROC)
/// (GLenum target, GLint level, GLint internalformat, GLsizei width, GLsizei
/// height, GLint border, GLenum format, GLenum type, const void *pixels);`
pub type PFNGLTEXIMAGE2DPROC = unsafe extern "system" fn(
    target: GLenum,
    level: GLint,
    internalformat: GLint,
    width: GLsizei,
    height: GLsizei,
    border: GLint,
    format: GLenum,
    type_: GLenum,
    pixels: *const c_void,
);

/// `glTexSubImage2D`: Replaces a rectangle of the pixels of a level of the
/// bound 2D texture.
/// `GL/glcorearb.h` (GL 1.1): `typedef void (APIENTRYP PFNGLTEXSUBIMAGE2DPROC)
/// (GLenum target, GLint level, GLint xoffset, GLint yoffset, GLsizei width,
/// GLsizei height, GLenum format, GLenum type, const void *pixels);`
pub type PFNGLTEXSUBIMAGE2DPROC = unsafe extern "system" fn(
    target: GLenum,
    level: GLint,
    xoffset: GLint,
    yoffset: GLint,
    width: GLsizei,
    height: GLsizei,
    format: GLenum,
    type_: GLenum,
    pixels: *const c_void,
);

/// `glTexParameteri`: Sets an integer parameter of the bound texture, such as
/// `GL_TEXTURE_MAX_LEVEL`.
/// `GL/glcorearb.h` (GL 1.0): `typedef void (APIENTRYP PFNGLTEXPARAMETERIPROC)
/// (GLenum target, GLenum pname, GLint param);`
pub type PFNGLTEXPARAMETERIPROC =
    unsafe extern "system" fn(target: GLenum, pname: GLenum, param: GLint);

// ---------------------------------------------------------------------------
// Functions: samplers
// ---------------------------------------------------------------------------

/// `glGenSamplers`: Writes `count` new sampler names to `samplers`.
/// `GL/glcorearb.h` (GL 3.3): `typedef void (APIENTRYP PFNGLGENSAMPLERSPROC)
/// (GLsizei count, GLuint *samplers);`
pub type PFNGLGENSAMPLERSPROC = unsafe extern "system" fn(count: GLsizei, samplers: *mut GLuint);

/// `glDeleteSamplers`: Deletes `count` samplers.
/// `GL/glcorearb.h` (GL 3.3): `typedef void (APIENTRYP PFNGLDELETESAMPLERSPROC)
/// (GLsizei count, const GLuint *samplers);`
pub type PFNGLDELETESAMPLERSPROC =
    unsafe extern "system" fn(count: GLsizei, samplers: *const GLuint);

/// `glBindSampler`: Binds a sampler to a texture unit, by the unit's number;
/// its filters and wrap modes then apply to that unit's texture. 0 unbinds.
/// `GL/glcorearb.h` (GL 3.3): `typedef void (APIENTRYP PFNGLBINDSAMPLERPROC)
/// (GLuint unit, GLuint sampler);`
pub type PFNGLBINDSAMPLERPROC = unsafe extern "system" fn(unit: GLuint, sampler: GLuint);

/// `glSamplerParameteri`: Sets an integer parameter of a sampler: a filter or a
/// wrap mode.
/// `GL/glcorearb.h` (GL 3.3): `typedef void (APIENTRYP
/// PFNGLSAMPLERPARAMETERIPROC) (GLuint sampler, GLenum pname, GLint param);`
pub type PFNGLSAMPLERPARAMETERIPROC =
    unsafe extern "system" fn(sampler: GLuint, pname: GLenum, param: GLint);

// ---------------------------------------------------------------------------
// Functions: buffers
// ---------------------------------------------------------------------------

/// `glGenBuffers`: Writes `n` new buffer names to `buffers`.
/// `GL/glcorearb.h` (GL 1.5): `typedef void (APIENTRYP PFNGLGENBUFFERSPROC)
/// (GLsizei n, GLuint *buffers);`
pub type PFNGLGENBUFFERSPROC = unsafe extern "system" fn(n: GLsizei, buffers: *mut GLuint);

/// `glDeleteBuffers`: Deletes `n` buffers.
/// `GL/glcorearb.h` (GL 1.5): `typedef void (APIENTRYP PFNGLDELETEBUFFERSPROC)
/// (GLsizei n, const GLuint *buffers);`
pub type PFNGLDELETEBUFFERSPROC = unsafe extern "system" fn(n: GLsizei, buffers: *const GLuint);

/// `glBindBuffer`: Binds a buffer to a target, such as `GL_ARRAY_BUFFER`.
/// `GL/glcorearb.h` (GL 1.5): `typedef void (APIENTRYP PFNGLBINDBUFFERPROC)
/// (GLenum target, GLuint buffer);`
pub type PFNGLBINDBUFFERPROC = unsafe extern "system" fn(target: GLenum, buffer: GLuint);

/// `glBufferData`: Gives the buffer bound to `target` a new store of `size`
/// bytes, filled from `data` unless that is null, with a hint of how it will be
/// used.
/// `GL/glcorearb.h` (GL 1.5): `typedef void (APIENTRYP PFNGLBUFFERDATAPROC)
/// (GLenum target, GLsizeiptr size, const void *data, GLenum usage);`
pub type PFNGLBUFFERDATAPROC =
    unsafe extern "system" fn(target: GLenum, size: GLsizeiptr, data: *const c_void, usage: GLenum);

/// `glBufferSubData`: Replaces `size` bytes of the store of the buffer bound to
/// `target`, from `offset`.
/// `GL/glcorearb.h` (GL 1.5): `typedef void (APIENTRYP PFNGLBUFFERSUBDATAPROC)
/// (GLenum target, GLintptr offset, GLsizeiptr size, const void *data);`
pub type PFNGLBUFFERSUBDATAPROC = unsafe extern "system" fn(
    target: GLenum,
    offset: GLintptr,
    size: GLsizeiptr,
    data: *const c_void,
);

/// `glBindBufferRange`: Binds `size` bytes of a buffer, from `offset`, to
/// binding point `index` of `target`: a uniform block's, for
/// `GL_UNIFORM_BUFFER`. The offset is a multiple of
/// `GL_UNIFORM_BUFFER_OFFSET_ALIGNMENT`.
/// `GL/glcorearb.h` (GL 3.0): `typedef void (APIENTRYP
/// PFNGLBINDBUFFERRANGEPROC) (GLenum target, GLuint index, GLuint buffer,
/// GLintptr offset, GLsizeiptr size);`
pub type PFNGLBINDBUFFERRANGEPROC = unsafe extern "system" fn(
    target: GLenum,
    index: GLuint,
    buffer: GLuint,
    offset: GLintptr,
    size: GLsizeiptr,
);

// ---------------------------------------------------------------------------
// Functions: vertex arrays
// ---------------------------------------------------------------------------

/// `glGenVertexArrays`: Writes `n` new vertex array names to `arrays`.
/// `GL/glcorearb.h` (GL 3.0): `typedef void (APIENTRYP
/// PFNGLGENVERTEXARRAYSPROC) (GLsizei n, GLuint *arrays);`
pub type PFNGLGENVERTEXARRAYSPROC = unsafe extern "system" fn(n: GLsizei, arrays: *mut GLuint);

/// `glDeleteVertexArrays`: Deletes `n` vertex arrays.
/// `GL/glcorearb.h` (GL 3.0): `typedef void (APIENTRYP
/// PFNGLDELETEVERTEXARRAYSPROC) (GLsizei n, const GLuint *arrays);`
pub type PFNGLDELETEVERTEXARRAYSPROC = unsafe extern "system" fn(n: GLsizei, arrays: *const GLuint);

/// `glBindVertexArray`: Binds a vertex array. The core profile draws only while
/// one is bound.
/// `GL/glcorearb.h` (GL 3.0): `typedef void (APIENTRYP
/// PFNGLBINDVERTEXARRAYPROC) (GLuint array);`
pub type PFNGLBINDVERTEXARRAYPROC = unsafe extern "system" fn(array: GLuint);

/// `glEnableVertexAttribArray`: Makes the bound vertex array read attribute
/// `index` from a buffer.
/// `GL/glcorearb.h` (GL 2.0): `typedef void (APIENTRYP
/// PFNGLENABLEVERTEXATTRIBARRAYPROC) (GLuint index);`
pub type PFNGLENABLEVERTEXATTRIBARRAYPROC = unsafe extern "system" fn(index: GLuint);

/// `glVertexAttribPointer`: Tells the bound vertex array where attribute
/// `index` comes from: the buffer bound to `GL_ARRAY_BUFFER` at this call, from
/// the byte offset `pointer`, with `stride` bytes from one element to the next.
/// `GL/glcorearb.h` (GL 2.0): `typedef void (APIENTRYP
/// PFNGLVERTEXATTRIBPOINTERPROC) (GLuint index, GLint size, GLenum type,
/// GLboolean normalized, GLsizei stride, const void *pointer);`
pub type PFNGLVERTEXATTRIBPOINTERPROC = unsafe extern "system" fn(
    index: GLuint,
    size: GLint,
    type_: GLenum,
    normalized: GLboolean,
    stride: GLsizei,
    pointer: *const c_void,
);

/// `glVertexAttribDivisor`: Makes attribute `index` advance once every
/// `divisor` instances, not once per vertex; 0 is once per vertex.
/// `GL/glcorearb.h` (GL 3.3): `typedef void (APIENTRYP
/// PFNGLVERTEXATTRIBDIVISORPROC) (GLuint index, GLuint divisor);`
pub type PFNGLVERTEXATTRIBDIVISORPROC = unsafe extern "system" fn(index: GLuint, divisor: GLuint);

// ---------------------------------------------------------------------------
// Functions: shaders and programs
// ---------------------------------------------------------------------------

/// `glCreateShader`: Makes a shader of the stage `type_`, and returns its name,
/// or 0 on failure.
/// `GL/glcorearb.h` (GL 2.0): `typedef GLuint (APIENTRYP PFNGLCREATESHADERPROC)
/// (GLenum type);`
pub type PFNGLCREATESHADERPROC = unsafe extern "system" fn(type_: GLenum) -> GLuint;

/// `glShaderSource`: Replaces the source of a shader with `count` strings. With
/// a null `length`, each string ends with a NUL; otherwise `length` holds the
/// length of each.
/// `GL/glcorearb.h` (GL 2.0): `typedef void (APIENTRYP PFNGLSHADERSOURCEPROC)
/// (GLuint shader, GLsizei count, const GLchar *const*string, const GLint *length);`
pub type PFNGLSHADERSOURCEPROC = unsafe extern "system" fn(
    shader: GLuint,
    count: GLsizei,
    string: *const *const GLchar,
    length: *const GLint,
);

/// `glCompileShader`: Compiles a shader. Its `GL_COMPILE_STATUS` then says
/// whether that worked, and its info log why not.
/// `GL/glcorearb.h` (GL 2.0): `typedef void (APIENTRYP PFNGLCOMPILESHADERPROC)
/// (GLuint shader);`
pub type PFNGLCOMPILESHADERPROC = unsafe extern "system" fn(shader: GLuint);

/// `glGetShaderiv`: Writes a parameter of a shader, such as `GL_COMPILE_STATUS`
/// or `GL_INFO_LOG_LENGTH`, to `params`.
/// `GL/glcorearb.h` (GL 2.0): `typedef void (APIENTRYP PFNGLGETSHADERIVPROC)
/// (GLuint shader, GLenum pname, GLint *params);`
pub type PFNGLGETSHADERIVPROC =
    unsafe extern "system" fn(shader: GLuint, pname: GLenum, params: *mut GLint);

/// `glGetShaderInfoLog`: Writes up to `bufSize` bytes of a shader's info log,
/// with a NUL at the end, to `infoLog`, and its length without the NUL to
/// `length` unless that is null.
/// `GL/glcorearb.h` (GL 2.0): `typedef void (APIENTRYP
/// PFNGLGETSHADERINFOLOGPROC) (GLuint shader, GLsizei bufSize, GLsizei *length,
/// GLchar *infoLog);`
pub type PFNGLGETSHADERINFOLOGPROC = unsafe extern "system" fn(
    shader: GLuint,
    bufSize: GLsizei,
    length: *mut GLsizei,
    infoLog: *mut GLchar,
);

/// `glDeleteShader`: Deletes a shader, or marks it to be deleted along with the
/// program it is attached to.
/// `GL/glcorearb.h` (GL 2.0): `typedef void (APIENTRYP PFNGLDELETESHADERPROC)
/// (GLuint shader);`
pub type PFNGLDELETESHADERPROC = unsafe extern "system" fn(shader: GLuint);

/// `glCreateProgram`: Makes a program, and returns its name, or 0 on failure.
/// `GL/glcorearb.h` (GL 2.0): `typedef GLuint (APIENTRYP
/// PFNGLCREATEPROGRAMPROC) (void);`
pub type PFNGLCREATEPROGRAMPROC = unsafe extern "system" fn() -> GLuint;

/// `glAttachShader`: Attaches a shader to a program, to be linked with it.
/// `GL/glcorearb.h` (GL 2.0): `typedef void (APIENTRYP PFNGLATTACHSHADERPROC)
/// (GLuint program, GLuint shader);`
pub type PFNGLATTACHSHADERPROC = unsafe extern "system" fn(program: GLuint, shader: GLuint);

/// `glLinkProgram`: Links the shaders attached to a program. Its
/// `GL_LINK_STATUS` then says whether that worked.
/// `GL/glcorearb.h` (GL 2.0): `typedef void (APIENTRYP PFNGLLINKPROGRAMPROC)
/// (GLuint program);`
pub type PFNGLLINKPROGRAMPROC = unsafe extern "system" fn(program: GLuint);

/// `glGetProgramiv`: Writes a parameter of a program, such as `GL_LINK_STATUS`
/// or `GL_INFO_LOG_LENGTH`, to `params`.
/// `GL/glcorearb.h` (GL 2.0): `typedef void (APIENTRYP PFNGLGETPROGRAMIVPROC)
/// (GLuint program, GLenum pname, GLint *params);`
pub type PFNGLGETPROGRAMIVPROC =
    unsafe extern "system" fn(program: GLuint, pname: GLenum, params: *mut GLint);

/// `glGetProgramInfoLog`: Writes up to `bufSize` bytes of a program's info log,
/// with a NUL at the end, to `infoLog`, and its length without the NUL to
/// `length` unless that is null.
/// `GL/glcorearb.h` (GL 2.0): `typedef void (APIENTRYP
/// PFNGLGETPROGRAMINFOLOGPROC) (GLuint program, GLsizei bufSize, GLsizei *length,
/// GLchar *infoLog);`
pub type PFNGLGETPROGRAMINFOLOGPROC = unsafe extern "system" fn(
    program: GLuint,
    bufSize: GLsizei,
    length: *mut GLsizei,
    infoLog: *mut GLchar,
);

/// `glDeleteProgram`: Deletes a program, or marks it to be deleted once it is
/// no longer in use.
/// `GL/glcorearb.h` (GL 2.0): `typedef void (APIENTRYP PFNGLDELETEPROGRAMPROC)
/// (GLuint program);`
pub type PFNGLDELETEPROGRAMPROC = unsafe extern "system" fn(program: GLuint);

/// `glUseProgram`: Makes a linked program the one that draws; 0 for none.
/// `GL/glcorearb.h` (GL 2.0): `typedef void (APIENTRYP PFNGLUSEPROGRAMPROC)
/// (GLuint program);`
pub type PFNGLUSEPROGRAMPROC = unsafe extern "system" fn(program: GLuint);

// ---------------------------------------------------------------------------
// Functions: uniforms
// ---------------------------------------------------------------------------

/// `glGetUniformLocation`: Returns the location of an active uniform, outside
/// any named block, of a linked program, by its name, which ends with a NUL.
/// Returns -1 for a name that is none, for a uniform that no stage uses (the
/// linker makes it inactive), and for one in a named block; `glUniform1i`
/// ignores -1.
/// `GL/glcorearb.h` (GL 2.0): `typedef GLint (APIENTRYP
/// PFNGLGETUNIFORMLOCATIONPROC) (GLuint program, const GLchar *name);`
pub type PFNGLGETUNIFORMLOCATIONPROC =
    unsafe extern "system" fn(program: GLuint, name: *const GLchar) -> GLint;

/// `glUniform1i`: Sets an `int` or sampler uniform of the program in use. A
/// sampler's value is the number of its texture unit.
/// `GL/glcorearb.h` (GL 2.0): `typedef void (APIENTRYP PFNGLUNIFORM1IPROC)
/// (GLint location, GLint v0);`
pub type PFNGLUNIFORM1IPROC = unsafe extern "system" fn(location: GLint, v0: GLint);

/// `glGetUniformBlockIndex`: Returns the index of a uniform block of a linked
/// program, by its name, which ends with a NUL, or `GL_INVALID_INDEX` if the
/// name is no active uniform block of the program.
/// `GL/glcorearb.h` (GL 3.1): `typedef GLuint (APIENTRYP
/// PFNGLGETUNIFORMBLOCKINDEXPROC) (GLuint program, const GLchar *uniformBlockName);`
pub type PFNGLGETUNIFORMBLOCKINDEXPROC =
    unsafe extern "system" fn(program: GLuint, uniformBlockName: *const GLchar) -> GLuint;

/// `glUniformBlockBinding`: Connects a uniform block of a program to a binding
/// point of `GL_UNIFORM_BUFFER`.
/// `GL/glcorearb.h` (GL 3.1): `typedef void (APIENTRYP
/// PFNGLUNIFORMBLOCKBINDINGPROC) (GLuint program, GLuint uniformBlockIndex,
/// GLuint uniformBlockBinding);`
pub type PFNGLUNIFORMBLOCKBINDINGPROC = unsafe extern "system" fn(
    program: GLuint,
    uniformBlockIndex: GLuint,
    uniformBlockBinding: GLuint,
);

/// `glGetActiveUniformBlockiv`: Writes a parameter of a uniform block of a
/// program, such as `GL_UNIFORM_BLOCK_DATA_SIZE`, to `params`.
/// `GL/glcorearb.h` (GL 3.1): `typedef void (APIENTRYP
/// PFNGLGETACTIVEUNIFORMBLOCKIVPROC) (GLuint program, GLuint uniformBlockIndex,
/// GLenum pname, GLint *params);`
pub type PFNGLGETACTIVEUNIFORMBLOCKIVPROC = unsafe extern "system" fn(
    program: GLuint,
    uniformBlockIndex: GLuint,
    pname: GLenum,
    params: *mut GLint,
);

// ---------------------------------------------------------------------------
// Functions: framebuffers
// ---------------------------------------------------------------------------

/// `glGenFramebuffers`: Writes `n` new framebuffer names to `framebuffers`.
/// `GL/glcorearb.h` (GL 3.0): `typedef void (APIENTRYP
/// PFNGLGENFRAMEBUFFERSPROC) (GLsizei n, GLuint *framebuffers);`
pub type PFNGLGENFRAMEBUFFERSPROC =
    unsafe extern "system" fn(n: GLsizei, framebuffers: *mut GLuint);

/// `glDeleteFramebuffers`: Deletes `n` framebuffers. A bound one is unbound
/// first, which binds the default framebuffer.
/// `GL/glcorearb.h` (GL 3.0): `typedef void (APIENTRYP
/// PFNGLDELETEFRAMEBUFFERSPROC) (GLsizei n, const GLuint *framebuffers);`
pub type PFNGLDELETEFRAMEBUFFERSPROC =
    unsafe extern "system" fn(n: GLsizei, framebuffers: *const GLuint);

/// `glBindFramebuffer`: Binds a framebuffer to draw to and read from
/// (`GL_FRAMEBUFFER`). 0 is the default framebuffer, the surface's: a window's
/// or a pbuffer's. A context without a surface has none, and drawing to it
/// fails with `GL_INVALID_FRAMEBUFFER_OPERATION`.
/// `GL/glcorearb.h` (GL 3.0): `typedef void (APIENTRYP
/// PFNGLBINDFRAMEBUFFERPROC) (GLenum target, GLuint framebuffer);`
pub type PFNGLBINDFRAMEBUFFERPROC = unsafe extern "system" fn(target: GLenum, framebuffer: GLuint);

/// `glFramebufferTexture2D`: Attaches a level of a 2D texture to an attachment
/// point of the bound framebuffer.
/// `GL/glcorearb.h` (GL 3.0): `typedef void (APIENTRYP
/// PFNGLFRAMEBUFFERTEXTURE2DPROC) (GLenum target, GLenum attachment, GLenum
/// textarget, GLuint texture, GLint level);`
pub type PFNGLFRAMEBUFFERTEXTURE2DPROC = unsafe extern "system" fn(
    target: GLenum,
    attachment: GLenum,
    textarget: GLenum,
    texture: GLuint,
    level: GLint,
);

/// `glCheckFramebufferStatus`: Returns whether the framebuffer bound to
/// `target` can be drawn to: `GL_FRAMEBUFFER_COMPLETE`, a reason why not, or 0
/// if the call fails.
/// `GL/glcorearb.h` (GL 3.0): `typedef GLenum (APIENTRYP
/// PFNGLCHECKFRAMEBUFFERSTATUSPROC) (GLenum target);`
pub type PFNGLCHECKFRAMEBUFFERSTATUSPROC = unsafe extern "system" fn(target: GLenum) -> GLenum;

/// `glReadPixels`: Writes a rectangle of the pixels of the bound read
/// framebuffer, from its bottom-left corner, to `pixels`, as `format` and
/// `type_` say: `height` rows, each padded to the pack alignment, 4 at first.
/// Rows of `GL_RGBA` and `GL_UNSIGNED_BYTE` are `4 * width` bytes, which needs
/// no padding.
/// `GL/glcorearb.h` (GL 1.0): `typedef void (APIENTRYP PFNGLREADPIXELSPROC)
/// (GLint x, GLint y, GLsizei width, GLsizei height, GLenum format, GLenum
/// type, void *pixels);`
pub type PFNGLREADPIXELSPROC = unsafe extern "system" fn(
    x: GLint,
    y: GLint,
    width: GLsizei,
    height: GLsizei,
    format: GLenum,
    type_: GLenum,
    pixels: *mut c_void,
);

// ---------------------------------------------------------------------------
// Functions: drawing
// ---------------------------------------------------------------------------

/// `glDrawArraysInstanced`: Draws `count` vertices from vertex `first`,
/// `instancecount` times.
/// `GL/glcorearb.h` (GL 3.1): `typedef void (APIENTRYP
/// PFNGLDRAWARRAYSINSTANCEDPROC) (GLenum mode, GLint first, GLsizei count,
/// GLsizei instancecount);`
pub type PFNGLDRAWARRAYSINSTANCEDPROC =
    unsafe extern "system" fn(mode: GLenum, first: GLint, count: GLsizei, instancecount: GLsizei);

/// `glDrawElementsInstancedBaseVertex`: Draws `count` indices of `type_`, from
/// the byte offset `indices` in the buffer bound to `GL_ELEMENT_ARRAY_BUFFER`,
/// `instancecount` times, with `basevertex` added to each index.
/// `GL/glcorearb.h` (GL 3.2): `typedef void (APIENTRYP
/// PFNGLDRAWELEMENTSINSTANCEDBASEVERTEXPROC) (GLenum mode, GLsizei count,
/// GLenum type, const void *indices, GLsizei instancecount, GLint basevertex);`
pub type PFNGLDRAWELEMENTSINSTANCEDBASEVERTEXPROC = unsafe extern "system" fn(
    mode: GLenum,
    count: GLsizei,
    type_: GLenum,
    indices: *const c_void,
    instancecount: GLsizei,
    basevertex: GLint,
);
