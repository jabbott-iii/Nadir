//! The checks that need no GPU: descriptions, regions, rectangles, and the
//! bounds of every draw. A draw that read outside a buffer would be
//! undefined behavior that GL does not always catch, so [`check_draw`] is
//! what the draw calls' safety rests on, for every read that the device sets
//! up. What a shader's own code indexes is the shader's to keep in bounds.

use core::ops::Range;

use nadir_core::{IRect, IVec2, UVec2};

use crate::GfxError;
use crate::types::{
    IndexFormat, MAX_TEXTURE_SLOTS, MAX_UNIFORM_BLOCKS, MAX_UNIFORM_BYTES, MAX_VERTEX_ATTRIBUTES,
    MAX_VERTEX_BUFFERS, PipelineDesc, VertexStep,
};

/// What the driver says when it gives no log.
pub(crate) const NO_LOG: &str = "(the driver gave no log)";

/// An info log as text, without the NUL and the white space at its end.
pub(crate) fn trim_log(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let text = text.trim_end_matches(|c: char| c == '\0' || c.is_whitespace());
    if text.is_empty() {
        NO_LOG.to_string()
    } else {
        text.to_string()
    }
}

/// Whether the first line of a GLSL source is exactly `#version 330 core`.
pub(crate) fn is_glsl_330_core(source: &str) -> bool {
    let line = source.split('\n').next().unwrap_or_default();
    line.strip_suffix('\r').unwrap_or(line) == "#version 330 core"
}

// ---------------------------------------------------------------------------
// Rectangles
// ---------------------------------------------------------------------------

/// The part of `rect` inside a target of `size`.
pub(crate) fn intersect(rect: IRect, size: UVec2) -> IRect {
    let clamp = |value: i32, max: u32| value.clamp(0, i32::try_from(max).unwrap_or(i32::MAX));
    let min = IVec2::new(clamp(rect.min.x, size.x), clamp(rect.min.y, size.y));
    let max = IVec2::new(clamp(rect.max.x, size.x), clamp(rect.max.y, size.y));
    IRect::new(min, IVec2::new(max.x.max(min.x), max.y.max(min.y)))
}

/// A rectangle of top-left target pixels as GL's `[x, y, width, height]`,
/// whose origin is the bottom left of a target `height` pixels tall. An
/// empty rectangle has a width or height of 0.
pub(crate) fn rect_to_gl(rect: IRect, height: u32) -> [i32; 4] {
    let to_i32 = |value: i64| {
        i32::try_from(value.clamp(i64::from(i32::MIN), i64::from(i32::MAX))).unwrap_or(0)
    };
    let width = (i64::from(rect.max.x) - i64::from(rect.min.x)).max(0);
    let rows = (i64::from(rect.max.y) - i64::from(rect.min.y)).max(0);
    let y = i64::from(height) - i64::from(rect.max.y);
    [
        to_i32(i64::from(rect.min.x)),
        to_i32(y),
        to_i32(width),
        to_i32(rows),
    ]
}

/// Reverses the order of the rows of an image, in place.
pub(crate) fn flip_rows(pixels: &mut [u8], row_bytes: usize) {
    if row_bytes == 0 {
        return;
    }
    let rows = pixels.len() / row_bytes;
    for top in 0..rows / 2 {
        let bottom = rows - 1 - top;
        let (upper, lower) = pixels.split_at_mut(bottom * row_bytes);
        upper[top * row_bytes..(top + 1) * row_bytes].swap_with_slice(&mut lower[..row_bytes]);
    }
}

// ---------------------------------------------------------------------------
// Resources
// ---------------------------------------------------------------------------

/// Checks that a texture of `size` can be made, with each side from 1 to
/// `max`.
pub(crate) fn check_texture_size(size: UVec2, max: u32) -> Result<(), GfxError> {
    let fits = |side: u32| (1..=max).contains(&side);
    if fits(size.x) && fits(size.y) {
        Ok(())
    } else {
        Err(GfxError::TooLarge { size, max })
    }
}

/// Checks that `region` lies inside a texture of `size`, and returns it as
/// `[x, y, width, height]`.
pub(crate) fn check_region(region: IRect, size: UVec2) -> Result<[u32; 4], GfxError> {
    let inside = |min: i32, max: i32, side: u32| {
        min >= 0 && max >= min && u32::try_from(max).is_ok_and(|max| max <= side)
    };
    if !(inside(region.min.x, region.max.x, size.x) && inside(region.min.y, region.max.y, size.y)) {
        return Err(GfxError::OutOfBounds("a texture region"));
    }
    // Each value was just checked to be from 0 to a `u32` side.
    let unsigned = |value: i32| u32::try_from(value).unwrap_or(0);
    Ok([
        unsigned(region.min.x),
        unsigned(region.min.y),
        unsigned(region.width()),
        unsigned(region.height()),
    ])
}

/// The byte offset of a write of `bytes` bytes at element `offset_elems` of
/// elements of `element_size` bytes, into a buffer of `size` bytes.
pub(crate) fn write_range(
    size: usize,
    offset_elems: usize,
    element_size: usize,
    bytes: usize,
) -> Result<usize, GfxError> {
    offset_elems
        .checked_mul(element_size)
        .filter(|offset| offset.checked_add(bytes).is_some_and(|end| end <= size))
        .ok_or(GfxError::OutOfBounds("a buffer write"))
}

/// Whether a GLSL source has an `#extension` directive, which could give its
/// shaders resources that no description names, and no draw checks. The
/// source is read as the preprocessor reads it: a backslash before a newline
/// joins the lines, and each comment is a space.
pub(crate) fn has_extension_directive(source: &str) -> bool {
    let joined = source.replace("\\\r\n", "").replace("\\\n", "");
    let mut text = String::with_capacity(joined.len());
    let mut chars = joined.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, chars.peek()) {
            ('/', Some('/')) => {
                while chars.next_if(|&c| c != '\n').is_some() {}
                text.push(' ');
            }
            ('/', Some('*')) => {
                chars.next();
                let mut last = ' ';
                for c in chars.by_ref() {
                    if last == '*' && c == '/' {
                        break;
                    }
                    // The lines go on, so that a directive after the
                    // comment is still found.
                    if c == '\n' {
                        text.push('\n');
                    }
                    last = c;
                }
                text.push(' ');
            }
            _ => text.push(c),
        }
    }
    text.lines().any(|line| {
        let Some(directive) = line.trim_start().strip_prefix('#') else {
            return false;
        };
        directive
            .trim_start()
            .strip_prefix("extension")
            .is_some_and(|rest| !rest.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_'))
    })
}

/// The widest vertex stride: GL 4.4's least `GL_MAX_VERTEX_ATTRIB_STRIDE`,
/// which drivers keep to in 3.3 contexts too.
const MAX_VERTEX_STRIDE: u32 = 2048;

/// Checks a pipeline description against the limits, without its shaders.
/// The error says what is wrong.
pub(crate) fn check_pipeline_desc(desc: &PipelineDesc<'_>) -> Result<(), String> {
    if !is_glsl_330_core(desc.shader.glsl_vs) || !is_glsl_330_core(desc.shader.glsl_fs) {
        return Err("the first line of each GLSL shader must be `#version 330 core`".into());
    }
    if has_extension_directive(desc.shader.glsl_vs) || has_extension_directive(desc.shader.glsl_fs)
    {
        return Err("GLSL 330 core shaders may not enable extensions (`#extension`)".into());
    }
    if desc.vertex_layouts.len() > MAX_VERTEX_BUFFERS as usize {
        return Err(format!(
            "more than {MAX_VERTEX_BUFFERS} vertex buffer slots"
        ));
    }
    let mut locations = 0u32;
    for (slot, layout) in desc.vertex_layouts.iter().enumerate() {
        if layout.stride == 0
            || !layout.stride.is_multiple_of(4)
            || layout.stride > MAX_VERTEX_STRIDE
        {
            return Err(format!(
                "vertex slot {slot}: the stride must be a multiple of 4 from 4 to \
                 {MAX_VERTEX_STRIDE}"
            ));
        }
        if layout.attributes.is_empty() {
            return Err(format!("vertex slot {slot} has no attributes"));
        }
        for attribute in layout.attributes {
            let end = u64::from(attribute.offset) + u64::from(attribute.format.size());
            if !attribute.offset.is_multiple_of(4) || end > u64::from(layout.stride) {
                return Err(format!(
                    "vertex slot {slot}: the attribute at location {} must be at a multiple of \
                     4 bytes, inside the stride",
                    attribute.location
                ));
            }
            if attribute.location >= MAX_VERTEX_ATTRIBUTES {
                return Err(format!(
                    "location {} is not below {MAX_VERTEX_ATTRIBUTES}",
                    attribute.location
                ));
            }
            if locations & (1 << attribute.location) != 0 {
                return Err(format!("location {} is used twice", attribute.location));
            }
            locations |= 1 << attribute.location;
        }
    }
    if desc.uniform_blocks.len() > MAX_UNIFORM_BLOCKS as usize {
        return Err(format!("more than {MAX_UNIFORM_BLOCKS} uniform blocks"));
    }
    let mut bindings = 0u32;
    for (i, block) in desc.uniform_blocks.iter().enumerate() {
        check_name(block.name, "a uniform block")?;
        if desc.uniform_blocks[..i]
            .iter()
            .any(|other| other.name == block.name)
        {
            return Err(format!("uniform block `{}` is named twice", block.name));
        }
        if block.binding >= MAX_UNIFORM_BLOCKS {
            return Err(format!(
                "uniform block `{}`: the binding must be below {MAX_UNIFORM_BLOCKS}",
                block.name
            ));
        }
        if bindings & (1 << block.binding) != 0 {
            return Err(format!("binding {} is used twice", block.binding));
        }
        bindings |= 1 << block.binding;
        let size = block.size as usize;
        if !size.is_multiple_of(16) || !(16..=MAX_UNIFORM_BYTES).contains(&size) {
            return Err(format!(
                "uniform block `{}`: the size must be a multiple of 16 from 16 to \
                 {MAX_UNIFORM_BYTES}",
                block.name
            ));
        }
    }
    if desc.textures.len() > MAX_TEXTURE_SLOTS as usize {
        return Err(format!("more than {MAX_TEXTURE_SLOTS} texture slots"));
    }
    for (i, name) in desc.textures.iter().enumerate() {
        check_name(name, "a sampler")?;
        if desc.textures[..i].contains(name) {
            return Err(format!("sampler `{name}` is named twice"));
        }
    }
    Ok(())
}

/// Checks a name that goes to GL as a C string.
fn check_name(name: &str, what: &str) -> Result<(), String> {
    if name.is_empty() || name.contains('\0') {
        return Err(format!(
            "{what} has no name, or a NUL in its name: `{name}`"
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Index buffers
// ---------------------------------------------------------------------------

/// The device's copy of an index buffer, and the smallest and largest index
/// in it, read as either type.
pub(crate) struct IndexShadow {
    bytes: Vec<u8>,
    u16_range: Option<(u32, u32)>,
    u32_range: Option<(u32, u32)>,
}

impl IndexShadow {
    /// A copy of a buffer of `size` bytes, which starts as zeros.
    pub(crate) fn new(size: usize) -> Result<Self, GfxError> {
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(size)
            .map_err(|_| GfxError::OutOfMemory("the device's copy of an index buffer"))?;
        bytes.resize(size, 0);
        let mut shadow = Self {
            bytes,
            u16_range: None,
            u32_range: None,
        };
        shadow.measure();
        Ok(shadow)
    }

    /// The bytes, which the buffer is created with.
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Copies a write of the buffer, which the caller checked is inside it.
    pub(crate) fn write(&mut self, offset: usize, data: &[u8]) {
        if let Some(target) = self.bytes.get_mut(offset..offset + data.len()) {
            target.copy_from_slice(data);
            self.measure();
        }
    }

    fn measure(&mut self) {
        self.u16_range = min_max(indices(&self.bytes, IndexFormat::U16, 0..u32::MAX));
        self.u32_range = min_max(indices(&self.bytes, IndexFormat::U32, 0..u32::MAX));
    }

    /// The smallest and largest index of the whole buffer, or `None` if it
    /// holds none.
    fn whole(&self, format: IndexFormat) -> Option<(u32, u32)> {
        match format {
            IndexFormat::U16 => self.u16_range,
            IndexFormat::U32 => self.u32_range,
        }
    }

    /// The smallest and largest of the indices `range`, which the caller
    /// checked are in the buffer.
    fn exact(&self, format: IndexFormat, range: Range<u32>) -> Option<(u32, u32)> {
        min_max(indices(&self.bytes, format, range))
    }
}

/// The indices of `range` in `bytes`, read as `format`.
fn indices(bytes: &[u8], format: IndexFormat, range: Range<u32>) -> impl Iterator<Item = u32> {
    let size = format.size();
    let start = range.start as usize;
    let count = range.end.saturating_sub(range.start) as usize;
    bytes
        .chunks_exact(size)
        .skip(start)
        .take(count)
        .map(move |index| match index {
            [a, b] => u32::from(u16::from_ne_bytes([*a, *b])),
            [a, b, c, d] => u32::from_ne_bytes([*a, *b, *c, *d]),
            _ => 0,
        })
}

fn min_max(values: impl Iterator<Item = u32>) -> Option<(u32, u32)> {
    values.fold(None, |range, value| match range {
        None => Some((value, value)),
        Some((min, max)) => Some((min.min(value), max.max(value))),
    })
}

// ---------------------------------------------------------------------------
// Uniforms
// ---------------------------------------------------------------------------

/// The bytes from one uniform block's data to the next in the uniform ring:
/// room for the largest block, at the offset alignment the driver requires.
pub(crate) fn ring_stride(alignment: usize) -> usize {
    let alignment = alignment.max(1);
    MAX_UNIFORM_BYTES.max(alignment).div_ceil(alignment) * alignment
}

// ---------------------------------------------------------------------------
// Draws
// ---------------------------------------------------------------------------

/// A vertex buffer slot of a pipeline, as a draw needs it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SlotLayout {
    pub(crate) stride: u32,
    pub(crate) step: VertexStep,
    /// The bytes of one record that the attributes read: the end of the last
    /// attribute.
    pub(crate) extent: u32,
}

/// A vertex buffer as a draw sees it: its size, and where the slot starts.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BoundVertices {
    pub(crate) size: usize,
    pub(crate) offset: usize,
}

/// An index buffer as a draw sees it.
pub(crate) struct BoundIndices<'a> {
    pub(crate) shadow: &'a IndexShadow,
    pub(crate) format: IndexFormat,
}

/// What a draw reads its vertices with.
pub(crate) enum DrawKind<'a> {
    /// Vertices `range`.
    Arrays(Range<u32>),
    /// The indices `range`, each plus `base_vertex`.
    Indexed {
        indices: BoundIndices<'a>,
        range: Range<u32>,
        base_vertex: i32,
    },
}

/// Checks that a draw reads only inside its buffers: for each vertex slot,
/// every vertex or instance record it reads, and for an indexed draw, every
/// index. `vertices[slot]` is the buffer set for `layouts[slot]`; a slot
/// that is not set is [`GfxError::Unbound`]. The ranges are not empty.
pub(crate) fn check_draw(
    layouts: &[SlotLayout],
    vertices: &[Option<BoundVertices>],
    kind: &DrawKind<'_>,
    instances: &Range<u32>,
) -> Result<(), GfxError> {
    // The largest vertex index that every per-vertex slot can read, and the
    // same for instances.
    let mut vertex_limit = u64::MAX;
    for (slot, layout) in layouts.iter().enumerate() {
        let bound = vertices
            .get(slot)
            .copied()
            .flatten()
            .ok_or(GfxError::Unbound {
                what: "vertex buffer",
                slot: slot as u32,
            })?;
        let last = last_record(bound, *layout).ok_or(GfxError::OutOfBounds(
            "a vertex buffer slot starts too close to the end of its buffer",
        ))?;
        match layout.step {
            VertexStep::Vertex => vertex_limit = vertex_limit.min(last),
            VertexStep::Instance => {
                if u64::from(instances.end - 1) > last {
                    return Err(GfxError::OutOfBounds("an instance range"));
                }
            }
        }
    }
    let (count, last_vertex) = match kind {
        DrawKind::Arrays(range) => {
            if range.start > i32::MAX as u32 {
                return Err(GfxError::OutOfBounds("a vertex range"));
            }
            (range.end - range.start, i64::from(range.end - 1))
        }
        DrawKind::Indexed {
            indices,
            range,
            base_vertex,
        } => {
            let size = indices.format.size() as u64;
            if u64::from(range.end) * size > indices.shadow.bytes().len() as u64 {
                return Err(GfxError::OutOfBounds("an index range"));
            }
            let base = i64::from(*base_vertex);
            let fits = |(min, max): (u32, u32)| {
                i64::from(min) + base >= 0
                    && u64::try_from(i64::from(max) + base).is_ok_and(|max| max <= vertex_limit)
            };
            // The whole buffer is measured once a write; only if that
            // fails are the indices of this draw read.
            let whole = indices.shadow.whole(indices.format);
            let (min, max) = match whole {
                Some(range) if fits(range) => range,
                _ => indices
                    .shadow
                    .exact(indices.format, range.clone())
                    .ok_or(GfxError::OutOfBounds("an index range"))?,
            };
            if i64::from(min) + base < 0 {
                return Err(GfxError::OutOfBounds("an index plus the base vertex"));
            }
            (range.end - range.start, i64::from(max) + base)
        }
    };
    if count > i32::MAX as u32 || instances.end - instances.start > i32::MAX as u32 {
        return Err(GfxError::OutOfBounds("a draw count"));
    }
    if u64::try_from(last_vertex).map_or(true, |last| last > vertex_limit) {
        return Err(GfxError::OutOfBounds("a vertex range"));
    }
    Ok(())
}

/// The largest record index of `layout` that reads inside `bound`, or `None`
/// if not even record 0 fits.
fn last_record(bound: BoundVertices, layout: SlotLayout) -> Option<u64> {
    let room = (bound.size as u64).checked_sub(bound.offset as u64)?;
    let room = room.checked_sub(u64::from(layout.extent))?;
    Some(room / u64::from(layout.stride))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        BlendMode, ShaderSource, TextureFormat, Topology, UniformBlockDesc, VertexAttribute,
        VertexFormat, VertexLayout,
    };

    #[test]
    fn a_log_loses_its_nul_and_trailing_space() {
        assert_eq!(trim_log(b"0:1(2): error: oops\n\0"), "0:1(2): error: oops");
        assert_eq!(trim_log(b""), NO_LOG);
        assert_eq!(trim_log(b"\0"), NO_LOG);
        assert_eq!(trim_log(b"bad \xFF byte"), "bad \u{FFFD} byte");
    }

    #[test]
    fn only_glsl_330_core_is_taken() {
        assert!(is_glsl_330_core("#version 330 core\nvoid main() {}"));
        assert!(is_glsl_330_core("#version 330 core\r\nvoid main() {}"));
        assert!(is_glsl_330_core("#version 330 core"));
        assert!(!is_glsl_330_core("\u{FEFF}#version 330 core\n"));
        assert!(!is_glsl_330_core("void main() {}"));
        assert!(!is_glsl_330_core("#version 330\n"));
        assert!(!is_glsl_330_core("#version 410 core\n"));
        assert!(!is_glsl_330_core(" #version 330 core\n"));
    }

    fn rect(x0: i32, y0: i32, x1: i32, y1: i32) -> IRect {
        IRect::new(IVec2::new(x0, y0), IVec2::new(x1, y1))
    }

    #[test]
    fn a_rectangle_from_the_top_left_becomes_one_from_the_bottom_left() {
        assert_eq!(rect_to_gl(rect(1, 0, 3, 1), 4), [1, 3, 2, 1]);
        assert_eq!(rect_to_gl(rect(0, 0, 8, 8), 8), [0, 0, 8, 8]);
        assert_eq!(rect_to_gl(rect(0, 6, 8, 8), 8), [0, 0, 8, 2]);
        // Empty or upside-down rectangles have no area.
        assert_eq!(rect_to_gl(rect(3, 3, 1, 1), 4)[2..], [0, 0]);
        // The extremes clamp instead of overflowing.
        let huge = rect_to_gl(rect(i32::MIN, i32::MIN, i32::MAX, i32::MAX), u32::MAX);
        assert_eq!(huge[2..], [i32::MAX, i32::MAX]);
    }

    #[test]
    fn a_rectangle_is_cut_to_its_target() {
        let size = UVec2::new(8, 4);
        assert_eq!(intersect(rect(-2, -2, 3, 3), size), rect(0, 0, 3, 3));
        assert_eq!(intersect(rect(6, 2, 20, 9), size), rect(6, 2, 8, 4));
        assert!(intersect(rect(10, 10, 12, 12), size).is_empty());
        assert!(intersect(rect(3, 3, 1, 1), size).is_empty());
    }

    #[test]
    fn rows_are_reversed_and_twice_is_nothing() {
        let mut pixels: Vec<u8> = (0..15).collect();
        flip_rows(&mut pixels, 3);
        assert_eq!(pixels, [12, 13, 14, 9, 10, 11, 6, 7, 8, 3, 4, 5, 0, 1, 2]);
        flip_rows(&mut pixels, 3);
        assert_eq!(pixels, (0..15).collect::<Vec<u8>>());
        let mut one = vec![7u8; 4];
        flip_rows(&mut one, 4);
        assert_eq!(one, [7; 4]);
        flip_rows(&mut [], 0);
    }

    #[test]
    fn texture_sizes_and_regions_are_checked() {
        assert!(check_texture_size(UVec2::new(1, 16384), 16384).is_ok());
        assert!(check_texture_size(UVec2::new(0, 4), 16384).is_err());
        assert!(check_texture_size(UVec2::new(16385, 4), 16384).is_err());
        let size = UVec2::new(4, 3);
        assert_eq!(check_region(rect(1, 1, 3, 2), size), Ok([1, 1, 2, 1]));
        assert_eq!(check_region(rect(0, 0, 4, 3), size), Ok([0, 0, 4, 3]));
        assert_eq!(check_region(rect(2, 2, 2, 2), size), Ok([2, 2, 0, 0]));
        assert!(check_region(rect(-1, 0, 2, 2), size).is_err());
        assert!(check_region(rect(0, 0, 5, 2), size).is_err());
        assert!(check_region(rect(2, 0, 1, 2), size).is_err());
    }

    #[test]
    fn a_buffer_write_must_fit_without_overflow() {
        assert_eq!(write_range(16, 1, 4, 12), Ok(4));
        assert_eq!(write_range(16, 0, 4, 16), Ok(0));
        assert!(write_range(16, 1, 4, 16).is_err());
        assert!(write_range(16, usize::MAX, 4, 4).is_err());
        assert!(write_range(16, 1, usize::MAX, 0).is_err());
    }

    const VS: &str = "#version 330 core\nvoid main() {}";

    fn desc<'a>(
        layouts: &'a [VertexLayout<'a>],
        blocks: &'a [UniformBlockDesc<'a>],
        textures: &'a [&'a str],
    ) -> PipelineDesc<'a> {
        PipelineDesc {
            label: "test",
            shader: ShaderSource {
                glsl_vs: VS,
                glsl_fs: VS,
                msl: "",
            },
            vertex_layouts: layouts,
            uniform_blocks: blocks,
            textures,
            blend: BlendMode::Opaque,
            topology: Topology::Triangles,
            target_format: TextureFormat::Rgba8,
        }
    }

    const POSITION: [VertexAttribute; 1] = [VertexAttribute {
        location: 0,
        format: VertexFormat::Float32x2,
        offset: 0,
    }];

    fn layout(stride: u32, attributes: &[VertexAttribute]) -> VertexLayout<'_> {
        VertexLayout {
            stride,
            step: VertexStep::Vertex,
            attributes,
        }
    }

    #[test]
    fn a_good_pipeline_description_passes() {
        let layouts = [layout(8, &POSITION)];
        let blocks = [UniformBlockDesc {
            name: "Camera",
            binding: 0,
            size: 64,
        }];
        assert_eq!(
            check_pipeline_desc(&desc(&layouts, &blocks, &["atlas"])),
            Ok(())
        );
    }

    #[test]
    fn bad_vertex_layouts_are_refused() {
        let at = |location, offset| VertexAttribute {
            location,
            format: VertexFormat::Float32x2,
            offset,
        };
        let cases: [&[VertexAttribute]; 4] = [
            &[at(0, 2)],
            &[at(0, 4)],
            &[at(0, 0), at(0, 0)],
            &[at(16, 0)],
        ];
        for attributes in cases {
            let layouts = [layout(8, attributes)];
            assert!(
                check_pipeline_desc(&desc(&layouts, &[], &[])).is_err(),
                "{attributes:?}"
            );
        }
        for stride in [0, 6] {
            let layouts = [layout(stride, &POSITION)];
            assert!(check_pipeline_desc(&desc(&layouts, &[], &[])).is_err());
        }
        let layouts = [layout(8, &[])];
        assert!(check_pipeline_desc(&desc(&layouts, &[], &[])).is_err());
        let layouts = [layout(8, &POSITION); 9];
        assert!(check_pipeline_desc(&desc(&layouts, &[], &[])).is_err());
    }

    #[test]
    fn bad_uniform_blocks_and_samplers_are_refused() {
        let block = |name, binding, size| UniformBlockDesc {
            name,
            binding,
            size,
        };
        let cases = [
            vec![block("A", 8, 16)],
            vec![block("A", 0, 16), block("B", 0, 16)],
            vec![block("A", 0, 16), block("A", 1, 16)],
            vec![block("A", 0, 8)],
            vec![block("A", 0, 272)],
            vec![block("A", 0, 24)],
            vec![block("", 0, 16)],
            vec![block("A\0", 0, 16)],
        ];
        for blocks in &cases {
            assert!(
                check_pipeline_desc(&desc(&[], blocks, &[])).is_err(),
                "{blocks:?}"
            );
        }
        assert!(check_pipeline_desc(&desc(&[], &[], &["a", "a"])).is_err());
        assert!(check_pipeline_desc(&desc(&[], &[], &["a\0"])).is_err());
        assert!(check_pipeline_desc(&desc(&[], &[], &["t"; 9])).is_err());
    }

    #[test]
    fn a_shader_without_the_version_line_is_refused() {
        let mut bad = desc(&[], &[], &[]);
        bad.shader.glsl_fs = "void main() {}";
        assert!(
            check_pipeline_desc(&bad)
                .unwrap_err()
                .contains("#version 330 core")
        );
    }

    #[test]
    fn the_index_shadow_knows_its_smallest_and_largest_index() {
        let mut shadow = IndexShadow::new(8).unwrap();
        assert_eq!(shadow.whole(IndexFormat::U16), Some((0, 0)));
        let indices: Vec<u8> = [5u16, 9, 2, 7]
            .iter()
            .flat_map(|i| i.to_ne_bytes())
            .collect();
        shadow.write(0, &indices);
        assert_eq!(shadow.whole(IndexFormat::U16), Some((2, 9)));
        assert_eq!(shadow.exact(IndexFormat::U16, 0..2), Some((5, 9)));
        assert_eq!(shadow.exact(IndexFormat::U16, 2..4), Some((2, 7)));
        // A write that lowers the values lowers the maximum.
        shadow.write(2, &1u16.to_ne_bytes());
        assert_eq!(shadow.whole(IndexFormat::U16), Some((1, 7)));
        let wide: Vec<u8> = [70_000u32, 3]
            .iter()
            .flat_map(|i| i.to_ne_bytes())
            .collect();
        shadow.write(0, &wide);
        assert_eq!(shadow.whole(IndexFormat::U32), Some((3, 70_000)));
    }

    #[test]
    fn the_ring_stride_fits_a_block_at_the_alignment() {
        assert_eq!(ring_stride(16), 256);
        assert_eq!(ring_stride(32), 256);
        assert_eq!(ring_stride(256), 256);
        assert_eq!(ring_stride(512), 512);
        assert_eq!(ring_stride(96), 288);
        assert_eq!(ring_stride(0), 256);
    }

    const QUAD: SlotLayout = SlotLayout {
        stride: 8,
        step: VertexStep::Vertex,
        extent: 8,
    };
    const COLOR: SlotLayout = SlotLayout {
        stride: 16,
        step: VertexStep::Instance,
        extent: 16,
    };

    fn bound(size: usize, offset: usize) -> Option<BoundVertices> {
        Some(BoundVertices { size, offset })
    }

    #[test]
    fn a_draw_reads_only_inside_its_vertex_buffers() {
        // 4 vertices of 8 bytes, and 3 instances of 16.
        let layouts = [QUAD, COLOR];
        let buffers = [bound(32, 0), bound(48, 0)];
        let ok = |vertices: Range<u32>, instances: Range<u32>| {
            check_draw(&layouts, &buffers, &DrawKind::Arrays(vertices), &instances)
        };
        assert_eq!(ok(0..4, 0..3), Ok(()));
        assert!(ok(0..5, 0..1).is_err());
        assert!(ok(1..5, 0..1).is_err());
        assert!(ok(0..4, 0..4).is_err());
        assert!(ok(0..4, 2..4).is_err());
        assert_eq!(ok(3..4, 2..3), Ok(()));
        assert!(ok(0..u32::MAX, 0..1).is_err());
        assert!(ok(0..1, 0..u32::MAX).is_err());
        // An offset counts.
        let buffers = [bound(32, 8), bound(48, 0)];
        assert!(check_draw(&layouts, &buffers, &DrawKind::Arrays(0..4), &(0..1)).is_err());
        assert!(check_draw(&layouts, &buffers, &DrawKind::Arrays(0..3), &(0..1)).is_ok());
        // A slot that starts past its buffer allows nothing.
        let buffers = [bound(32, 40), bound(48, 0)];
        assert!(check_draw(&layouts, &buffers, &DrawKind::Arrays(0..1), &(0..1)).is_err());
        // A slot left unset.
        let missing = check_draw(
            &layouts,
            &[bound(32, 0), None],
            &DrawKind::Arrays(0..1),
            &(0..1),
        );
        assert_eq!(
            missing,
            Err(GfxError::Unbound {
                what: "vertex buffer",
                slot: 1
            })
        );
    }

    fn shadow_of(indices: &[u16]) -> IndexShadow {
        let mut shadow = IndexShadow::new(indices.len() * 2).unwrap();
        let bytes: Vec<u8> = indices.iter().flat_map(|i| i.to_ne_bytes()).collect();
        shadow.write(0, &bytes);
        shadow
    }

    #[test]
    fn an_indexed_draw_reads_only_its_indices_and_the_vertices_they_name() {
        let layouts = [QUAD];
        let buffers = [bound(64, 0)]; // 8 vertices
        let shadow = shadow_of(&[0, 1, 2, 2, 1, 3, 9, 9]);
        let draw = |range: Range<u32>, base_vertex: i32| {
            let kind = DrawKind::Indexed {
                indices: BoundIndices {
                    shadow: &shadow,
                    format: IndexFormat::U16,
                },
                range,
                base_vertex,
            };
            check_draw(&layouts, &buffers, &kind, &(0..1))
        };
        // Indices 0 to 3 plus 4 reach vertex 7, the last.
        assert_eq!(draw(0..6, 4), Ok(()));
        assert!(draw(0..6, 5).is_err());
        // Index 9 is past the vertices, but only if it is drawn.
        assert!(draw(0..8, 0).is_err());
        assert_eq!(draw(0..6, 0), Ok(()));
        // A negative base vertex may not take an index below 0.
        assert!(draw(0..6, -1).is_err());
        assert_eq!(draw(3..6, -1), Ok(()));
        // Past the end of the index buffer.
        assert!(draw(0..9, 0).is_err());
        assert!(draw(0..u32::MAX, 0).is_err());
    }

    #[test]
    fn a_slot_with_less_room_than_one_record_allows_nothing() {
        // 4 bytes are left after the offset, and a record needs 8.
        let kind = DrawKind::Arrays(0..1);
        assert!(check_draw(&[QUAD], &[bound(32, 28)], &kind, &(0..1)).is_err());
        assert!(check_draw(&[QUAD], &[bound(32, 24)], &kind, &(0..1)).is_ok());
    }

    #[test]
    fn the_shortest_vertex_slot_limits_the_draw() {
        let wide = SlotLayout {
            stride: 16,
            step: VertexStep::Vertex,
            extent: 16,
        };
        let draw = |buffers: &[Option<BoundVertices>], end| {
            check_draw(&[QUAD, wide], buffers, &DrawKind::Arrays(0..end), &(0..1))
        };
        // 8 vertices in the first slot, 2 in the second.
        assert!(draw(&[bound(64, 0), bound(32, 0)], 2).is_ok());
        assert!(draw(&[bound(64, 0), bound(32, 0)], 3).is_err());
        // 4 in the first, 10 in the second.
        assert!(draw(&[bound(32, 0), bound(160, 0)], 4).is_ok());
        assert!(draw(&[bound(32, 0), bound(160, 0)], 5).is_err());
    }

    #[test]
    fn a_stride_over_2048_is_refused() {
        let attributes = [VertexAttribute {
            location: 0,
            format: VertexFormat::Float32,
            offset: 0,
        }];
        for (stride, ok) in [(2048, true), (2052, false)] {
            let layouts = [layout(stride, &attributes)];
            assert_eq!(
                check_pipeline_desc(&desc(&layouts, &[], &[])).is_ok(),
                ok,
                "{stride}"
            );
        }
    }

    #[test]
    fn an_extension_directive_is_found_however_it_is_written() {
        let found = [
            "#extension GL_ARB_shader_storage_buffer_object : require",
            "  #  extension X : enable",
            "#/**/extension X : enable",
            "#ext\\\nension X : enable",
            "/* a\nb */ # extension X : enable",
            "void f();\n#extension X : enable\n",
        ];
        for source in found {
            assert!(has_extension_directive(source), "{source:?}");
        }
        let not_found = [
            "// #extension X : enable",
            "/* #extension X : enable */",
            "int extension_count;",
            "#extensions",
            "#define EXT 1",
        ];
        for source in not_found {
            assert!(!has_extension_directive(source), "{source:?}");
        }
        let mut with_extension = desc(&[], &[], &[]);
        with_extension.shader.glsl_fs = "#version 330 core\n#extension X : require\n";
        assert!(
            check_pipeline_desc(&with_extension)
                .unwrap_err()
                .contains("#extension")
        );
    }

    #[test]
    fn a_draw_with_no_vertex_slots_has_no_vertex_limit() {
        assert_eq!(
            check_draw(&[], &[], &DrawKind::Arrays(0..6), &(0..1)),
            Ok(())
        );
        assert!(check_draw(&[], &[], &DrawKind::Arrays(u32::MAX - 1..u32::MAX), &(0..1)).is_err());
    }
}
