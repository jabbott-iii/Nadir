# Subsystem: `nadir-gfx` (render hardware interface)

A deliberately small GPU abstraction: exactly what a 2D renderer needs, and
nothing more. It has two backends:

- **OpenGL 3.3 core:** all OSes, M2 onward.
- **Metal:** macOS, M10 onward.

See ADR-0004 and ADR-0005.

## Scope

**In:**

- 2D textures (RGBA8 / sRGB8_A8 / R8)
- render targets (color only, plus an optional depth-less MSAA-free setup)
- vertex/index/instance buffers
- uniform blocks (small, per draw or per pass)
- pipelines (shader pair, vertex layout, blend mode, primitive type)
- scissor and viewport
- instanced draws
- readback for tests
- GPU timer queries (optional)

**Out:**

- compute
- depth/stencil (2D ordering uses sort keys)
- 3D textures and cubemaps
- mipmaps (pixel art does not want them; smooth-scaled assets may get them
  later)
- multiple render targets
- bindless resources
- threading (main thread only)

## API sketch

Departures from the first sketch are recorded in ADR-0016: the device holds
no borrow of its surface, frames take the surface, and pass calls return
`Result`.

```rust
pub struct Device { backend: Backend /* GL now; an enum Gl | Metal from M10 */ }   // !Send, !Sync, one per thread

impl Device {
    pub fn new(surface: GpuSurface<'_>) -> Result<Self, GfxError>;   // keeps the context's id, not a borrow
    pub fn new_headless() -> Result<Self, GfxError>;     // owns an offscreen context: tests and tools (M3.2)
    pub fn caps(&self) -> &Caps;                          // backend, vendor, renderer, version, max texture size (timer queries: M7.3)

    pub fn create_texture(&mut self, desc: &TextureDesc, data: Option<&[u8]>) -> Result<TextureId, GfxError>;
    pub fn update_texture(&mut self, id: TextureId, region: IRect, data: &[u8]) -> Result<(), GfxError>;
    pub fn create_buffer(&mut self, desc: &BufferDesc) -> Result<BufferId, GfxError>;
    pub fn write_buffer<T: Pod>(&mut self, id: BufferId, offset_elems: usize, data: &[T]) -> Result<(), GfxError>;
    pub fn create_render_target(&mut self, size: UVec2, format: TextureFormat) -> Result<RenderTargetId, GfxError>;
    pub fn create_pipeline(&mut self, desc: &PipelineDesc<'_>) -> Result<PipelineId, GfxError>;
    pub fn destroy(&mut self, res: impl Into<ResourceId>);

    pub fn begin_frame(&mut self, surface: GpuSurface<'_>) -> Result<(), GfxError>;   // the drawable size; Metal: acquire drawable
    pub fn begin_pass(&mut self, target: PassTarget, clear: Option<LinearColor>) -> Result<Pass<'_>, GfxError>;
    pub fn end_frame(&mut self, surface: GpuSurface<'_>) -> Result<(), GfxError>;     // present (GL swap / Metal present+commit)
    pub fn begin_offscreen_frame(&mut self) -> Result<(), GfxError>;   // a frame on new_headless's own context
    pub fn end_offscreen_frame(&mut self) -> Result<(), GfxError>;

    pub fn read_pixels(&mut self, target: RenderTargetId) -> Result<Vec<u8>, GfxError>; // RGBA8, top-left origin (tests)
}

impl Pass<'_> {
    pub fn set_viewport(&mut self, r: IRect);                   // top-left target pixels
    pub fn set_scissor(&mut self, r: Option<IRect>);
    pub fn set_pipeline(&mut self, p: PipelineId) -> Result<(), GfxError>;
    pub fn set_vertex_buffer(&mut self, slot: u32, b: BufferId, offset_bytes: usize) -> Result<(), GfxError>;
    pub fn set_index_buffer(&mut self, b: BufferId, format: IndexFormat) -> Result<(), GfxError>;
    pub fn set_texture(&mut self, slot: u32, t: impl Into<TextureRef>, sampler: Sampler) -> Result<(), GfxError>;   // TextureRef: Texture | RenderTarget
    pub fn set_uniforms<T: Pod>(&mut self, block: u32, data: &[T]) -> Result<(), GfxError>;   // block = binding; ≤ 256 bytes, std140-ordered
    pub fn draw(&mut self, vertices: Range<u32>, instances: Range<u32>) -> Result<(), GfxError>;
    pub fn draw_indexed(&mut self, indices: Range<u32>, base_vertex: i32, instances: Range<u32>) -> Result<(), GfxError>;
}   // Drop ends the pass.

/// Plain-old-data that can be viewed as bytes.
/// # Safety
/// Implementors must be `#[repr(C)]`, contain no padding bytes, no pointers or references,
/// and every bit pattern must be valid.
pub unsafe trait Pod: Copy + 'static {}
pub fn bytes_of<T: Pod>(slice: &[T]) -> &[u8];
```

- Resource IDs are generational (`GenArena` from core). Using a destroyed ID
  is a `GfxError::StaleHandle`, not undefined behavior.
- **Liveness (ADR-0016):** every call that reaches the GPU first checks that
  the device's context still exists (`GlContextId::is_current`), and fails
  with `ContextLost` once it is gone. A context has one device.
- **Frames and passes:** a frame is `begin_frame(surface)`, passes, then
  `end_frame(surface)`; resources are made, changed and destroyed outside
  passes. A pass starts with nothing set (pipeline, buffers, textures,
  uniforms), as a Metal encoder does, and with the whole target as its
  viewport. A `Surface` pass on a surface with no pixels (a headless
  context, a minimized window) makes no GPU call, and its draws do
  nothing.
- **Setters fail at once** on a stale id or an argument out of range, and
  leave that setting unset. **Draws check what they read** before calling
  the GPU: a set pipeline; every vertex slot set with a vertex buffer;
  every vertex and instance record, and every index plus the base vertex,
  inside its buffer; every uniform block set with exactly its size; every
  texture slot set. A pass may not sample its own target
  (`FeedbackLoop`), nor use a pipeline of another target format. The
  shaders' own code is trusted: an array index out of bounds in a shader is
  undefined behavior that no check covers.
- **Buffers:** `BufferDesc { label, kind: Vertex | Index, usage: Static |
  Dynamic, size }`. An index buffer starts as zeros, and the device keeps a
  copy of it to check indexed draws. What is written to a `Dynamic` buffer
  is good for the frame it is written in.
- **Instancing:** GL 3.3 has no base instance, so a draw of instances
  `n..m` starts the instance slots at record `n`. Shaders read instance data
  only through attributes, never through `gl_InstanceID` (or
  `[[instance_id]]`).
- **Orientation:** a texture's first row of data is `v = 0`. `read_pixels`
  gives rows from the top. On GL a render target is stored from its bottom
  row, so sampled, its `v = 0` is the bottom of what was drawn (clip space
  `y = -1`): shaders that sample render targets (blit, post) account for
  it.
- **R8** samples as `(r, 0, 0, 1)`, and reads back as `(r, 0, 0, 255)`.
- `Pod` is implemented **only inside `gfx`**:
  - for `u8`, `u16`, `u32`, `i32` and `f32`
  - for arrays of these
  - for gfx-internal `#[repr(C)]` structs, each with a size assertion proving
    there is no padding

  Other crates deny `unsafe`, so they pack data into `Vec<f32>`/`Vec<u32>`
  (see `render.md`, "Instance data").

### Pipeline description

```rust
pub struct PipelineDesc<'a> {
    pub label: &'a str,
    pub shader: ShaderSource<'a>,                 // per-backend source chosen at runtime
    pub vertex_layouts: &'a [VertexLayout<'a>],   // per slot: stride, step (Vertex|Instance), attributes
    pub uniform_blocks: &'a [UniformBlockDesc<'a>],   // name + binding index + size
    pub textures: &'a [&'a str],                  // sampler names in slot order
    pub blend: BlendMode,                         // Opaque | Alpha | PremultipliedAlpha | Additive | Multiply
    pub topology: Topology,                       // Triangles | TriangleStrip | Lines
    pub target_format: TextureFormat,
}

pub struct ShaderSource<'a> {
    pub glsl_vs: &'a str, pub glsl_fs: &'a str,   // GLSL 330 core
    pub msl: &'a str,                             // one MSL source with vs_main / fs_main
}
```

## OpenGL 3.3 backend notes

- **Loader:** a `gl_fns!` macro lists the 60 functions we use, each with its
  `nadir_sys::gl` type (a compile-time check pairs name and type). It
  builds a struct of function pointers, loaded through
  `GlContext::get_proc_address`, every one before the first call. A missing
  function is a hard error that names it. Under libglvnd every `gl` name
  gives a stub, so the check that guards there is the version check: 3.3
  core, and nothing beyond it without an extension check.
- **Calls:** each GL function has one safe wrapper with one `unsafe`
  block, on a token that exists only after the liveness check; wrappers
  that pass pointers check the lengths. Only the two draws stay `unsafe`,
  and their one call site follows the draw checks.
- **Uploads:** `GL_UNPACK_ALIGNMENT` stays 1, so rows of any width are
  tightly packed. Every texture has `GL_TEXTURE_MAX_LEVEL` 0.
- **State cache:** track the bound program, VAO, textures per unit, blend
  state, viewport and scissor to avoid redundant calls. The cache is
  invalidated at `begin_frame`.
- **VAOs:** one VAO per pipeline. Attribute pointers are re-specified when the
  vertex buffer or its offset changes (the VAO's bindings are kept per
  pipeline, by buffer id). Instancing uses `glVertexAttribDivisor`.
- **Blend modes**, as `glBlendFuncSeparate` factors (color, then alpha),
  with `FUNC_ADD`. Metal bakes the same values:

  | Mode | Color | Alpha |
  | :- | :- | :- |
  | `Opaque` | blending off | |
  | `Alpha` | `SRC_ALPHA, ONE_MINUS_SRC_ALPHA` | `ONE, ONE_MINUS_SRC_ALPHA` |
  | `PremultipliedAlpha` | `ONE, ONE_MINUS_SRC_ALPHA` | `ONE, ONE_MINUS_SRC_ALPHA` |
  | `Additive` | `SRC_ALPHA, ONE` | `ZERO, ONE` |
  | `Multiply` | `DST_COLOR, ZERO` | `ZERO, ONE` |
- **Uniform blocks:**
  - GLSL 330 has no `layout(binding = N)`. After linking, call
    `glGetUniformBlockIndex` + `glUniformBlockBinding` for each block.
  - Rust structs use std140 rules: `vec2` aligns to 8, `vec4` to 16, arrays
    to 16. Every uniform struct has a size assertion.
  - A `UniformBlockDesc { name, binding, size }` names each block; its size
    (a multiple of 16, up to 256) must be the size the compiler gives it,
    and the description must name every block the shaders use. Blocks are
    found by name: drivers number them differently.
  - Uniform data goes into a 64 KiB ring, block after block at the driver's
    offset alignment, which gets new storage each frame and whenever it
    fills; nothing is allocated per draw.
- **Samplers:**
  - After linking, set each sampler uniform to its unit with `glUniform1i`,
    once.
  - Filtering and wrap use sampler objects, which are core in 3.3: one per
    `Sampler` variant (nearest/linear × clamp/repeat), bound per unit with
    `glBindSampler`.
- **Errors:**
  - Shader compile/link failures return `GfxError::Shader { label, stage, log }`
    with the driver log. The first line of each GLSL source must be
    `#version 330 core`, and no `#extension` directive may follow: an
    extension could give a shader resources that no draw checks. A vertex
    stride is at most 2048 bytes.
  - In debug builds (`cfg(debug_assertions)`, ADR-0016), `glGetError` is
    checked after every call, and an error is `GfxError::Gl { call, code }`.
  - In release it's checked once per frame, and the first failure is logged
    with the last command name.
  - In every build it is also checked before each draw, which is refused if
    a state call failed unseen, and before and after each call that
    allocates, which fails on an error an earlier call left. Out of memory,
    wherever it is seen, loses the device: every later call is
    `ContextLost`.
- **Readback:** `glReadPixels`, then flip vertically.
- **Color space:** decided by ADR-0013.
  - Textures are uploaded as `SRGB8_ALPHA8`; blending happens in linear space
    with `GL_FRAMEBUFFER_SRGB` enabled on the default framebuffer and on
    sRGB render targets.
  - If this causes visible differences from artists' expectations in pixel
    art (alpha-blended edges), switch to "everything sRGB-encoded,
    blend in sRGB". That change is one ADR plus a flag in `Caps`.

## Metal backend notes (M10)

- Messages go through `msg_send!` from `nadir-sys`. Hold every object with
  a retain/release RAII wrapper (`ObjcRetained`).
- Pipelines become `MTLRenderPipelineState`; the blend mode is baked into the
  pipeline (unlike GL, where it's set at bind time).
- Uniforms: `setVertexBytes:length:atIndex:` / `setFragmentBytes:...` (data ≤
  4 KB).
- Per frame:
  1. `nextDrawable`
  2. one command buffer
  3. one `MTLRenderCommandEncoder` per pass
  4. `presentDrawable:`, then `commit`
- Triple-buffer dynamic buffers with a semaphore (`dispatch_semaphore` via
  libSystem FFI), so the CPU never overwrites data the GPU is reading.
- Readback: blit to a shared-storage texture, `waitUntilCompleted`,
  `getBytes:`.
- MSL buffer indices: vertex buffers 0..7, uniform blocks 8..15. Texture and
  sampler indices match the slot order.

## Shader conventions (shared by both backends)

- Shader sources live in `crates/render/shaders/<name>/`:
  `<name>.vert.glsl`, `<name>.frag.glsl`, `<name>.metal`.
- They are included with `include_str!`. In dev builds they hot-reload from
  disk.
- Every shader has a Rust "interface" constant: attributes with locations,
  uniform blocks with sizes, texture slots. Both sources must agree with it.
  A parity test parses the GLSL `layout(location = N) in` lines and the MSL
  `[[attribute(N)]]` annotations and checks both against the constant.
- As built (M3.3), `sprite` and `blit` exist, in `crates/render`. Render's
  parity test checks each `layout(location = N) in` line of their GLSL
  against the interface constant, and that each uniform block and sampler
  is declared. MSL comes with M10.3.
- Expected programs:
  - `sprite` (instanced quads)
  - `tile_chunk`
  - `text` (alpha-mask glyphs, tint)
  - `debug_lines`
  - `blit` (integer upscale with letterbox)
  - `post` (color grade, fade)
  - later: `light2d`
- Keep the total under 20; past that, revisit ADR-0005.

## Tests

- **Golden scenes per backend** (M2.4, ADR-0017): tests in
  `tests/headless/golden.rs` that `cargo xtask golden` compares with
  `tests/golden/` (`docs/04-quality.md`):
  - clear color
  - textured quad: nearest, linear, and repeat through an indexed draw of
    a texture `update_texture` changed
  - alpha blending of overlapping quads, in each blend mode
  - 1000 instanced quads, in two draws
  - scissor and viewport
  - render-target round trip

  Integer upscale with letterbox is a `render` scene (M3.3), drawn with its
  `blit` program.
- **Parity test** for shader interfaces (pure Rust; runs everywhere).
- **Stale-handle test:** destroy, then use → error, not crash.
- **GL tests (M2.3):** behind the `headless-test` feature, on an offscreen
  context: the draw checks, readback, orientation, samplers, sRGB,
  blending, instancing, uniforms, the state cache and shader errors. The
  golden scenes run in the same binary. The shader parity test is render's
  (M3.3).
- **Headless devices (M3.2):** `Device::new_headless` makes a device that
  owns its offscreen context, so that crates which may not depend on
  `nadir-platform`, such as render, can test on a GPU. Asset types that
  need the GPU are tested in the crate that defines them.
  - Its frames are `begin_offscreen_frame` and `end_offscreen_frame`, and it
    draws into render targets.
  - Where this system has no offscreen context yet, it fails with
    `GfxError::NoOffscreenContext`, which tests skip on. Other failures
    carry the platform's error in `GfxError::OffscreenContext`.
- **Benchmark:** 10k instanced sprites, upload plus draw time.
