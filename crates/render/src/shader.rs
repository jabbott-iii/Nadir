//! The renderer's shaders and their interfaces.
//!
//! Each program's GLSL lives in `shaders/<name>/` and is included here.
//! Its interface constant names every vertex attribute, uniform block and
//! sampler; the pipelines are built from it, and the parity test checks the
//! GLSL against it. The Metal sources come with roadmap M10.3.

use nadir_gfx::{ShaderSource, UniformBlockDesc, VertexAttribute, VertexFormat};

/// A vertex attribute of a shader: its name, location and format.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Attribute {
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "only the parity test reads the names")
    )]
    pub(crate) name: &'static str,
    pub(crate) location: u32,
    pub(crate) format: VertexFormat,
}

/// What a program reads.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Interface {
    /// The program's name: its directory under `shaders/`.
    pub(crate) name: &'static str,
    pub(crate) vertex: &'static str,
    pub(crate) fragment: &'static str,
    /// The per-instance attributes, in the order they are packed.
    pub(crate) instance: &'static [Attribute],
    /// The per-vertex attributes: the unit quad.
    pub(crate) vertex_attributes: &'static [Attribute],
    pub(crate) uniform_blocks: &'static [UniformBlockDesc<'static>],
    pub(crate) textures: &'static [&'static str],
}

impl Interface {
    pub(crate) fn source(&self) -> ShaderSource<'static> {
        ShaderSource {
            glsl_vs: self.vertex,
            glsl_fs: self.fragment,
            msl: "",
        }
    }
}

const fn attribute(name: &'static str, location: u32, format: VertexFormat) -> Attribute {
    Attribute {
        name,
        location,
        format,
    }
}

/// Instanced sprites: one quad each, textured and tinted.
pub(crate) const SPRITE: Interface = Interface {
    name: "sprite",
    vertex: include_str!("../shaders/sprite/sprite.vert.glsl"),
    fragment: include_str!("../shaders/sprite/sprite.frag.glsl"),
    instance: &[
        attribute("position", 0, VertexFormat::Float32x2),
        attribute("size", 1, VertexFormat::Float32x2),
        attribute("pivot", 2, VertexFormat::Float32x2),
        attribute("uv_rect", 3, VertexFormat::Float32x4),
        attribute("color", 4, VertexFormat::Float32x4),
        attribute("rotation", 5, VertexFormat::Float32),
    ],
    vertex_attributes: &[attribute("corner", 6, VertexFormat::Float32x2)],
    uniform_blocks: &[UniformBlockDesc {
        name: "Camera",
        binding: 0,
        size: 16,
    }],
    textures: &["sprite_texture"],
};

/// The integer upscale: the virtual image over the viewport.
pub(crate) const BLIT: Interface = Interface {
    name: "blit",
    vertex: include_str!("../shaders/blit/blit.vert.glsl"),
    fragment: include_str!("../shaders/blit/blit.frag.glsl"),
    instance: &[],
    vertex_attributes: &[
        attribute("position", 0, VertexFormat::Float32x2),
        attribute("uv", 1, VertexFormat::Float32x2),
    ],
    uniform_blocks: &[],
    textures: &["source"],
};

/// Every program, for the parity test.
#[cfg(test)]
const ALL: [Interface; 2] = [SPRITE, BLIT];

/// The vertex attributes of `attributes`, packed one after another.
pub(crate) fn packed<const N: usize>(attributes: &[Attribute]) -> ([VertexAttribute; N], u32) {
    let mut out = [VertexAttribute {
        location: 0,
        format: VertexFormat::Float32,
        offset: 0,
    }; N];
    let mut offset = 0;
    for (slot, attribute) in out.iter_mut().zip(attributes) {
        *slot = VertexAttribute {
            location: attribute.location,
            format: attribute.format,
            offset,
        };
        offset += attribute.format.size();
    }
    (out, offset)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SPRITE_INSTANCE_FLOATS;

    /// The GLSL type of a format.
    fn glsl_type(format: VertexFormat) -> &'static str {
        match format {
            VertexFormat::Float32 => "float",
            VertexFormat::Float32x2 => "vec2",
            VertexFormat::Float32x3 => "vec3",
            VertexFormat::Float32x4 => "vec4",
            _ => "?",
        }
    }

    /// Every `layout(location = N) in TYPE NAME;` of `source`.
    fn inputs(source: &str) -> Vec<(u32, String, String)> {
        source
            .lines()
            .filter_map(|line| {
                let rest = line.trim().strip_prefix("layout(location = ")?;
                let (location, rest) = rest.split_once(") in ")?;
                let (kind, name) = rest.trim_end_matches(';').split_once(' ')?;
                Some((location.parse().ok()?, kind.to_string(), name.to_string()))
            })
            .collect()
    }

    #[test]
    fn the_glsl_declares_exactly_the_interface() {
        for program in ALL {
            let mut expected: Vec<(u32, String, String)> = program
                .instance
                .iter()
                .chain(program.vertex_attributes)
                .map(|a| {
                    (
                        a.location,
                        glsl_type(a.format).to_string(),
                        a.name.to_string(),
                    )
                })
                .collect();
            expected.sort();
            let mut found = inputs(program.vertex);
            found.sort();
            assert_eq!(found, expected, "{}", program.name);
            for block in program.uniform_blocks {
                let declaration = format!("uniform {} {{", block.name);
                assert!(
                    program.vertex.contains(&declaration),
                    "{} {}",
                    program.name,
                    block.name
                );
            }
            for texture in program.textures {
                let declaration = format!("uniform sampler2D {texture};");
                assert!(
                    program.fragment.contains(&declaration),
                    "{} {texture}",
                    program.name
                );
            }
        }
    }

    #[test]
    fn the_parser_reads_a_declaration() {
        assert_eq!(
            inputs("layout(location = 3) in vec4 uv_rect;\n  layout(location = 12) in float a;"),
            [
                (3, "vec4".to_string(), "uv_rect".to_string()),
                (12, "float".to_string(), "a".to_string()),
            ]
        );
    }

    #[test]
    fn the_sprite_instance_is_its_floats() {
        let (attributes, stride) = packed::<6>(SPRITE.instance);
        assert_eq!(stride, SPRITE_INSTANCE_FLOATS as u32 * 4);
        let offsets: Vec<u32> = attributes.iter().map(|a| a.offset / 4).collect();
        // The float each attribute starts at, as `push_instance` packs them.
        assert_eq!(offsets, [0, 2, 4, 6, 10, 14]);
    }
}
