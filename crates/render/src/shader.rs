// Copyright 2026 Joseph Anthony Abbott III
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! The renderer's shaders and their interfaces.
//!
//! Each program's GLSL lives in `shaders/<name>/` and is included here.
//! Its interface constant names every vertex attribute, uniform block and
//! sampler; the pipelines are built from it, and [`Interface::check`]
//! checks GLSL against it: the included sources in the tests, and sources
//! given to [`Renderer::reload_program`](crate::Renderer::reload_program)
//! before they are built. The Metal sources come with roadmap M10.3.

use std::iter::Peekable;
use std::str::Chars;

use nadir_gfx::{BlendMode, ShaderSource, UniformBlockDesc, VertexAttribute, VertexFormat};

/// The folder of the renderer's GLSL sources: this crate's `shaders/`,
/// where it was compiled. Each [`Program`]'s files are under it, at
/// [`Program::vertex_file`] and [`Program::fragment_file`].
///
/// The renderer includes the sources when it is compiled, and reads no
/// file. The folder is for a game that watches the files and gives them to
/// [`Renderer::reload_program`](crate::Renderer::reload_program) when they
/// change, as the engine's facade does in development builds, under its
/// `dev` feature (roadmap M5.8). It is the path on the machine that compiled the crate: for a
/// crate taken from crates.io, a folder in cargo's registry, which cargo
/// does not rebuild when its files change, so that the files edited there
/// and the sources compiled in drift apart. Edit the shaders of a path dependency, or of a
/// `[patch]`, instead.
pub const SHADER_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/shaders");

/// One of the renderer's programs, for
/// [`Renderer::reload_program`](crate::Renderer::reload_program).
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Program {
    /// Instanced sprites, textured and tinted.
    Sprite,
    /// Instanced tiles, a chunk of a tile layer at a time.
    TileChunk,
    /// The integer upscale of the virtual image into the output.
    Blit,
    /// Instanced glyphs of texts: the sprite's quads, with the texture's
    /// alpha as a mask in the tint's color.
    Text,
}

impl Program {
    /// Every program.
    pub const ALL: &'static [Self] = &[Self::Sprite, Self::TileChunk, Self::Blit, Self::Text];

    /// Its name: its folder under [`SHADER_DIR`], and its pipeline's label.
    pub fn name(self) -> &'static str {
        self.interface().name
    }

    /// Its vertex shader's file, under [`SHADER_DIR`], such as
    /// `sprite/sprite.vert.glsl`.
    pub fn vertex_file(self) -> &'static str {
        match self {
            Self::Sprite => "sprite/sprite.vert.glsl",
            Self::TileChunk => "tile_chunk/tile_chunk.vert.glsl",
            Self::Blit => "blit/blit.vert.glsl",
            Self::Text => "text/text.vert.glsl",
        }
    }

    /// Its fragment shader's file, under [`SHADER_DIR`], such as
    /// `sprite/sprite.frag.glsl`.
    pub fn fragment_file(self) -> &'static str {
        match self {
            Self::Sprite => "sprite/sprite.frag.glsl",
            Self::TileChunk => "tile_chunk/tile_chunk.frag.glsl",
            Self::Blit => "blit/blit.frag.glsl",
            Self::Text => "text/text.frag.glsl",
        }
    }

    pub(crate) fn interface(self) -> &'static Interface {
        match self {
            Self::Sprite => &SPRITE,
            Self::TileChunk => &TILE_CHUNK,
            Self::Blit => &BLIT,
            Self::Text => &TEXT,
        }
    }
}

/// A vertex attribute of a shader: its name, location and format.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Attribute {
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
    /// The per-instance attributes, in the order they are packed: vertex
    /// buffer slot 0. A program with none has only the per-vertex ones.
    pub(crate) instance: &'static [Attribute],
    /// The per-vertex attributes: the unit quad, in the slot after the
    /// instances'.
    pub(crate) vertex_attributes: &'static [Attribute],
    pub(crate) uniform_blocks: &'static [UniformBlockDesc<'static>],
    pub(crate) textures: &'static [&'static str],
    pub(crate) blend: BlendMode,
}

impl Interface {
    /// The sources it was compiled with.
    pub(crate) fn source(&self) -> ShaderSource<'static> {
        ShaderSource {
            glsl_vs: self.vertex,
            glsl_fs: self.fragment,
            msl: "",
        }
    }

    /// Checks that GLSL sources declare nothing that differs from this
    /// interface:
    ///
    /// - each `in` variable of the vertex shader is one of its attributes,
    ///   with its location, and no other layout qualifier, its type and its
    ///   name;
    /// - each uniform block of either shader is one of its blocks;
    /// - each other uniform of either shader is one of its samplers, a
    ///   `sampler2D`, unless it has a value of its own (`= …`);
    /// - the fragment shader has one output, an `out vec4` at location 0
    ///   and index 0, and writes no `gl_FragColor` or `gl_FragData`.
    ///
    /// An attribute, a block or a sampler that the sources leave out is
    /// taken: the device enables an attribute the program does not read,
    /// and warns of a block or a sampler it does not use. The device checks
    /// the rest when it builds the program: that the sources compile and
    /// link, and the blocks' sizes. A block's members are checked by its
    /// size alone.
    ///
    /// The declarations are read at the top level, as written once the
    /// preprocessor has joined the lines a `\` ends, made each comment a
    /// space and each newline one, and the directives are left out, but
    /// before it would expand a macro or choose a branch of an `#if`: a
    /// declaration a macro makes is not seen, and every branch is read.
    /// Sources whose branches do not each hold whole declarations, with the
    /// braces as deep at the end of each, are refused, as are braces that
    /// do not pair up.
    ///
    /// # Errors
    ///
    /// What differs, each difference once, joined by `; `.
    pub(crate) fn check(&self, vertex: &str, fragment: &str) -> Result<(), String> {
        let vertex = declarations(vertex);
        let fragment = declarations(fragment);
        let mut problems = Vec::new();
        for (stage, found) in [("vertex", &vertex), ("fragment", &fragment)] {
            if !found.readable {
                problems.push(format!(
                    "the {stage} shader's braces, or the branches of its `#if`s, do not hold \
                     whole declarations as written, so its declarations cannot be read"
                ));
            }
        }

        let attributes = || self.instance.iter().chain(self.vertex_attributes);
        let mut inputs = vertex.inputs;
        inputs.sort_unstable();
        // The same declaration twice comes from two branches of an `#if`.
        inputs.dedup();
        for input in &inputs {
            let location = input.qualifier("location").flatten();
            let fits = attributes().any(|attribute| {
                input.layout.len() == 1
                    && location == Some(attribute.location)
                    && glsl_type(attribute.format) == input.kind
                    && attribute.name == input.name
            });
            if fits {
                continue;
            }
            let declared = input.declaration("in");
            let mut problem = if input.layout.is_empty() {
                format!("`{declared}` has no location, and is not one of the program's attributes")
            } else {
                format!("`{declared}` is not one of the program's attributes")
            };
            let near = attributes()
                .find(|attribute| attribute.name == input.name)
                .or_else(|| attributes().find(|attribute| Some(attribute.location) == location));
            if let Some(attribute) = near {
                problem.push_str(&format!(
                    " (it has `layout(location = {}) in {} {};`)",
                    attribute.location,
                    glsl_type(attribute.format),
                    attribute.name
                ));
            }
            problems.push(problem);
        }

        let mut blocks = vertex.blocks;
        blocks.extend(fragment.blocks);
        blocks.sort_unstable();
        blocks.dedup();
        for name in blocks {
            if !self.uniform_blocks.iter().any(|block| block.name == name) {
                let names: Vec<String> = self
                    .uniform_blocks
                    .iter()
                    .map(|block| format!("`{}`", block.name))
                    .collect();
                problems.push(if names.is_empty() {
                    format!("uniform block `{name}` is not one of the program's, which has none")
                } else {
                    format!(
                        "uniform block `{name}` is not one of the program's (it has {})",
                        names.join(", ")
                    )
                });
            }
        }

        let mut uniforms = vertex.uniforms;
        uniforms.extend(fragment.uniforms);
        uniforms.sort_unstable();
        uniforms.dedup();
        for (kind, name) in uniforms {
            if kind == "sampler2D" && self.textures.contains(&name.as_str()) {
                continue;
            }
            let mut problem = format!(
                "`uniform {kind} {name};` is not one of the program's: the renderer sets only \
                 its uniform blocks and its samplers"
            );
            if self.textures.contains(&name.as_str()) {
                problem.push_str(&format!(" (`{name}` is a `sampler2D`)"));
            }
            problems.push(problem);
        }

        let mut outputs = fragment.outputs;
        outputs.sort_unstable();
        outputs.dedup();
        let is_color = |output: &Variable| {
            output.kind == "vec4"
                && !output.name.contains('[')
                && output.layout.iter().all(|(key, value)| {
                    (key.eq_ignore_ascii_case("location") || key.eq_ignore_ascii_case("index"))
                        && value.as_deref().and_then(integer) == Some(0)
                })
        };
        match outputs.as_slice() {
            [output] if is_color(output) => {}
            [] => problems.push(
                "the fragment shader has no output: it must have one `out vec4`, at location 0"
                    .to_string(),
            ),
            many => {
                let declared: Vec<String> = many
                    .iter()
                    .map(|output| format!("`{}`", output.declaration("out")))
                    .collect();
                problems.push(format!(
                    "the fragment shader has {}, and must have one `out vec4`, at location 0: \
                     the renderer draws into one color",
                    declared.join(", ")
                ));
            }
        }
        for name in fragment.built_in_outputs {
            problems.push(format!(
                "the fragment shader writes `{name}`: the renderer draws only its `out vec4`"
            ));
        }

        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems.join("; "))
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
    blend: BlendMode::Alpha,
};

/// Instanced tiles: one quad each, from a chunk's instance buffer, with
/// the tile picked from the layer's tile set and the frame's time.
pub(crate) const TILE_CHUNK: Interface = Interface {
    name: "tile_chunk",
    vertex: include_str!("../shaders/tile_chunk/tile_chunk.vert.glsl"),
    fragment: include_str!("../shaders/tile_chunk/tile_chunk.frag.glsl"),
    instance: &[
        attribute("cell", 0, VertexFormat::Float32x2),
        attribute("tile", 1, VertexFormat::Float32x4),
    ],
    vertex_attributes: &[attribute("corner", 2, VertexFormat::Float32x2)],
    uniform_blocks: &[
        UniformBlockDesc {
            name: "Camera",
            binding: 0,
            size: 16,
        },
        UniformBlockDesc {
            name: "TileLayer",
            binding: 1,
            size: 48,
        },
    ],
    textures: &["tile_texture"],
    blend: BlendMode::Alpha,
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
    blend: BlendMode::Opaque,
};

/// Instanced glyphs: the sprite's instances and quad, with the texture's
/// alpha as a mask in the tint's color (`docs/subsystems/render.md`,
/// "Text").
pub(crate) const TEXT: Interface = Interface {
    name: "text",
    vertex: include_str!("../shaders/text/text.vert.glsl"),
    fragment: include_str!("../shaders/text/text.frag.glsl"),
    instance: SPRITE.instance,
    vertex_attributes: SPRITE.vertex_attributes,
    uniform_blocks: SPRITE.uniform_blocks,
    textures: &["text_texture"],
    blend: BlendMode::Alpha,
};

/// The vertex attributes of `attributes`, packed one after another, and
/// the bytes from one record to the next. Past `attributes`, the array
/// holds zeros.
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

/// The GLSL type of a format.
fn glsl_type(format: VertexFormat) -> &'static str {
    match format {
        VertexFormat::Float32 => "float",
        VertexFormat::Float32x2 => "vec2",
        VertexFormat::Float32x3 => "vec3",
        VertexFormat::Float32x4 => "vec4",
        // A format gfx adds later needs its type here; until then, no
        // declaration matches it.
        _ => "?",
    }
}

/// `source` as the GLSL preprocessor reads it before it runs its
/// directives, in one pass that never reads again what it made: each `\`
/// before a newline joins the two lines; each newline is a `\n`, `\r\n`,
/// `\n\r` and a `\r` alone each being one, as GLSL has it; each comment is
/// a space; and a `"` starts a string, to the next `"` or the end of the
/// line, in which no comment starts. As Mesa's preprocessor does, the line
/// of a directive it does not take apart, such as `#pragma`, `#error` or
/// `#extension`, keeps the rest of its text as it is.
fn preprocessed(source: &str) -> String {
    // `\r\n` and `\n\r` are one newline: the second of the pair goes with
    // the first.
    fn pair(first: char, chars: &mut Peekable<Chars<'_>>) {
        chars.next_if_eq(&if first == '\r' { '\n' } else { '\r' });
    }
    // Past a block comment whose `/*` was read.
    fn comment(chars: &mut Peekable<Chars<'_>>) {
        let mut last = ' ';
        for c in chars.by_ref() {
            if last == '*' && c == '/' {
                break;
            }
            last = c;
        }
    }
    let mut lines = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\'
            && let Some(newline) = chars.next_if(|&c| c == '\r' || c == '\n')
        {
            pair(newline, &mut chars);
        } else if c == '\r' || c == '\n' {
            pair(c, &mut chars);
            lines.push('\n');
        } else {
            lines.push(c);
        }
    }

    let mut text = String::with_capacity(lines.len());
    let mut chars = lines.chars().peekable();
    // Whether only spaces and comments have come since the last newline.
    let mut line_start = true;
    while let Some(c) = chars.next() {
        match (c, chars.peek()) {
            ('\n', _) => {
                text.push('\n');
                line_start = true;
            }
            ('/', Some('/')) => {
                while chars.next_if(|&c| c != '\n').is_some() {}
                text.push(' ');
            }
            ('/', Some('*')) => {
                chars.next();
                comment(&mut chars);
                text.push(' ');
            }
            ('"', _) => {
                text.push('"');
                while let Some(c) = chars.next_if(|&c| c != '"' && c != '\n') {
                    text.push(c);
                }
                if chars.next_if_eq(&'"').is_some() {
                    text.push('"');
                }
                line_start = false;
            }
            ('#', _) if line_start => {
                // The directive's name, after spaces and comments.
                let mut ahead = chars.clone();
                loop {
                    while ahead.next_if(|&c| is_space(c) && c != '\n').is_some() {}
                    let mut opening = ahead.clone();
                    if opening.next() != Some('/') || opening.next() != Some('*') {
                        break;
                    }
                    ahead = opening;
                    comment(&mut ahead);
                }
                let mut name = String::new();
                while let Some(c) = ahead.next_if(|&c| c.is_ascii_alphanumeric() || c == '_') {
                    name.push(c);
                }
                text.push('#');
                let taken_apart = matches!(
                    name.as_str(),
                    "" | "define"
                        | "undef"
                        | "if"
                        | "ifdef"
                        | "ifndef"
                        | "elif"
                        | "else"
                        | "endif"
                        | "line"
                        | "version"
                );
                if !taken_apart {
                    chars = ahead;
                    text.push(' ');
                    text.push_str(&name);
                    while let Some(c) = chars.next_if(|&c| c != '\n') {
                        text.push(c);
                    }
                }
                line_start = false;
            }
            (c, _) => {
                line_start &= is_space(c);
                text.push(c);
            }
        }
    }
    text
}

/// Whether GLSL reads `c` as a space between tokens.
fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\x0B' | '\x0C')
}

/// What a conditional directive does to its group of branches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Branch {
    /// `#if`, `#ifdef` or `#ifndef`: a group starts.
    If,
    /// `#elif` or `#else`: another branch of it.
    Else,
    /// `#endif`: the group ends.
    End,
}

/// A piece of GLSL: a word (a name, a keyword, or a number up to a `.`),
/// one other character, or a conditional directive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Token<'a> {
    Word(&'a str),
    Mark(char),
    Branch(Branch),
}

/// The tokens of `text`, a [`preprocessed`] source. Of its directives,
/// each line whose first character, after spaces, is `#`, only the
/// conditional ones are kept, each as a [`Token::Branch`].
fn tokens(text: &str) -> Vec<Token<'_>> {
    let mut out = Vec::new();
    for line in text.split('\n') {
        if let Some(directive) = line.trim_start_matches(is_space).strip_prefix('#') {
            let name = directive.trim_start_matches(is_space);
            let end = name
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(name.len());
            match &name[..end] {
                "if" | "ifdef" | "ifndef" => out.push(Token::Branch(Branch::If)),
                "elif" | "else" => out.push(Token::Branch(Branch::Else)),
                "endif" => out.push(Token::Branch(Branch::End)),
                _ => {}
            }
            continue;
        }
        let mut rest = line;
        while let Some(c) = rest.chars().next() {
            if is_space(c) {
                rest = &rest[c.len_utf8()..];
            } else if c.is_ascii_alphanumeric() || c == '_' {
                let length = rest
                    .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                    .unwrap_or(rest.len());
                out.push(Token::Word(&rest[..length]));
                rest = &rest[length..];
            } else {
                out.push(Token::Mark(c));
                rest = &rest[c.len_utf8()..];
            }
        }
    }
    out
}

/// The text of `tokens`, with nothing between them.
fn text(tokens: &[Token<'_>]) -> String {
    let mut text = String::new();
    for &token in tokens {
        match token {
            Token::Word(word) => text.push_str(word),
            Token::Mark(mark) => text.push(mark),
            Token::Branch(_) => {}
        }
    }
    text
}

/// `tokens` cut at each `mark` outside parentheses and brackets.
fn split_outside<'t, 'a>(tokens: &'t [Token<'a>], mark: char) -> Vec<&'t [Token<'a>]> {
    let mut parts = Vec::new();
    let mut depth = 0_usize;
    let mut start = 0;
    for (index, &token) in tokens.iter().enumerate() {
        match token {
            Token::Mark('(' | '[') => depth += 1,
            Token::Mark(')' | ']') => depth = depth.saturating_sub(1),
            Token::Mark(found) if found == mark && depth == 0 => {
                parts.push(&tokens[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    parts.push(&tokens[start..]);
    parts
}

/// One qualifier of a layout: its name as written, and its value as
/// written, if it has one.
type Qualifier = (String, Option<String>);

/// A variable a shader takes in or gives out.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Variable {
    /// Its layout's qualifiers.
    layout: Vec<Qualifier>,
    kind: String,
    /// Its name, with any array size.
    name: String,
}

impl Variable {
    /// The value of its layout's qualifier `key`, of any case, read as a
    /// GLSL integer: `None` without that qualifier, and `Some(None)` with a
    /// value that is not an integer, or no value.
    fn qualifier(&self, key: &str) -> Option<Option<u32>> {
        self.layout
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(key))
            .map(|(_, value)| value.as_deref().and_then(integer))
    }

    /// Its declaration, such as `layout(location = 6) in vec2 corner;`.
    fn declaration(&self, storage: &str) -> String {
        let mut declared = String::new();
        if !self.layout.is_empty() {
            let qualifiers: Vec<String> = self
                .layout
                .iter()
                .map(|(key, value)| match value {
                    Some(value) => format!("{key} = {value}"),
                    None => key.clone(),
                })
                .collect();
            declared = format!("layout({}) ", qualifiers.join(", "));
        }
        declared.push_str(&format!("{storage} {} {};", self.kind, self.name));
        declared
    }
}

/// What a GLSL source declares at its top level.
#[derive(Debug, PartialEq, Eq)]
struct Declarations {
    /// The `in` variables (`attribute` ones too).
    inputs: Vec<Variable>,
    /// The `out` variables.
    outputs: Vec<Variable>,
    /// The uniform blocks' names.
    blocks: Vec<String>,
    /// The other uniforms without a value of their own: type, and name with
    /// any array size. A struct's type is `struct`.
    uniforms: Vec<(String, String)>,
    /// The built-in outputs of GLSL before 1.30 it names anywhere:
    /// `gl_FragColor` and `gl_FragData`.
    built_in_outputs: Vec<String>,
    /// Whether its braces pair up, and each branch of its `#if`s holds whole
    /// statements and leaves the braces as deep as the others, as written.
    readable: bool,
}

/// The declarations of `source`: each statement at the top level, up to
/// its `;`, or to the `{` of a body, which is passed over.
fn declarations(source: &str) -> Declarations {
    let text = preprocessed(source);
    let mut out = Declarations {
        inputs: Vec::new(),
        outputs: Vec::new(),
        blocks: Vec::new(),
        uniforms: Vec::new(),
        built_in_outputs: Vec::new(),
        readable: true,
    };
    let mut statement = Vec::new();
    let mut depth = 0_usize;
    // Each group of branches open: the depth it starts at, and the depth
    // its branches so far end at.
    let mut groups: Vec<(usize, Option<usize>)> = Vec::new();
    for token in tokens(&text) {
        if let Token::Word(word @ ("gl_FragColor" | "gl_FragData")) = token {
            out.built_in_outputs.push(word.to_string());
        }
        match (depth, token) {
            (_, Token::Branch(branch)) => {
                if depth == 0 && !statement.is_empty() {
                    out.readable = false;
                }
                match branch {
                    Branch::If => groups.push((depth, None)),
                    Branch::Else => match groups.last_mut() {
                        Some((start, end)) => {
                            if end.is_some_and(|end| end != depth) {
                                out.readable = false;
                            }
                            *end = Some(depth);
                            depth = *start;
                        }
                        None => out.readable = false,
                    },
                    Branch::End => match groups.pop() {
                        Some((start, end)) => {
                            if end.unwrap_or(start) != depth {
                                out.readable = false;
                            }
                        }
                        None => out.readable = false,
                    },
                }
            }
            (0, Token::Mark(';')) => {
                declare(&statement, false, &mut out);
                statement.clear();
            }
            (0, Token::Mark('{')) => {
                declare(&statement, true, &mut out);
                statement.clear();
                depth = 1;
            }
            (0, Token::Mark('}')) => out.readable = false,
            (0, token) => statement.push(token),
            (_, Token::Mark('{')) => depth += 1,
            (_, Token::Mark('}')) => depth -= 1,
            _ => {}
        }
    }
    if depth != 0 || !groups.is_empty() {
        out.readable = false;
    }
    out.built_in_outputs.sort_unstable();
    out.built_in_outputs.dedup();
    out
}

/// The value of a GLSL integer: decimal, octal after a `0`, or hexadecimal
/// after `0x`, with an optional `u`.
fn integer(text: &str) -> Option<u32> {
    let digits = text.strip_suffix(['u', 'U']).unwrap_or(text);
    if let Some(hex) = digits
        .strip_prefix("0x")
        .or_else(|| digits.strip_prefix("0X"))
    {
        u32::from_str_radix(hex, 16).ok()
    } else if let Some(octal) = digits.strip_prefix('0').filter(|octal| !octal.is_empty()) {
        u32::from_str_radix(octal, 8).ok()
    } else {
        digits.parse().ok()
    }
}

/// Adds what one statement declares to `out`: an `in` or `out` variable,
/// a uniform, or, before a body, a uniform block or a uniform struct.
/// Anything else, such as a function, a struct type or a block of `in` or
/// `out` variables, declares nothing the check reads.
fn declare(statement: &[Token<'_>], opens_body: bool, out: &mut Declarations) {
    let mut rest = statement;
    let mut layout = Vec::new();
    let mut storage = None;
    loop {
        match rest {
            [Token::Word("layout"), Token::Mark('('), tail @ ..] => {
                let end = tail
                    .iter()
                    .position(|&token| token == Token::Mark(')'))
                    .unwrap_or(tail.len());
                for part in tail[..end].split(|&token| token == Token::Mark(',')) {
                    layout.push(match part {
                        [Token::Word(key)] => ((*key).to_string(), None),
                        [Token::Word(key), Token::Mark('='), value @ ..] => {
                            ((*key).to_string(), Some(text(value)))
                        }
                        _ => (text(part), None),
                    });
                }
                rest = tail.get(end + 1..).unwrap_or(&[]);
            }
            // `attribute`, from before GLSL 1.30, is an `in` of the vertex
            // shader.
            [
                Token::Word(word @ ("in" | "attribute" | "out" | "uniform")),
                tail @ ..,
            ] => {
                storage = Some(if *word == "attribute" { "in" } else { *word });
                rest = tail;
            }
            [
                Token::Word(
                    "flat" | "smooth" | "noperspective" | "centroid" | "invariant" | "highp"
                    | "mediump" | "lowp" | "const",
                ),
                tail @ ..,
            ] => rest = tail,
            _ => break,
        }
    }
    let [Token::Word(kind), names @ ..] = rest else {
        return;
    };
    if opens_body {
        match (storage, *kind) {
            (Some("uniform"), "struct") => {
                let name = text(names);
                let name = if name.is_empty() {
                    "{…}".to_string()
                } else {
                    format!("{name} {{…}}")
                };
                out.uniforms.push(("struct".to_string(), name));
            }
            (Some("uniform"), _) if names.is_empty() => out.blocks.push((*kind).to_string()),
            _ => {}
        }
        return;
    }
    for declared in split_outside(names, ',') {
        let parts = split_outside(declared, '=');
        let name = parts.first().map_or_else(String::new, |name| text(name));
        let has_value = parts.len() > 1;
        let list = match storage {
            Some("in") => &mut out.inputs,
            Some("out") => &mut out.outputs,
            Some("uniform") if !has_value => {
                out.uniforms.push(((*kind).to_string(), name));
                continue;
            }
            _ => continue,
        };
        list.push(Variable {
            layout: layout.clone(),
            kind: (*kind).to_string(),
            name,
        });
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::*;
    use crate::{SPRITE_INSTANCE_FLOATS, TILE_INSTANCE_FLOATS};

    /// A variable as (its location, type, name).
    type Read = (Option<u32>, String, String);

    fn read(variables: &[Variable]) -> Vec<Read> {
        variables
            .iter()
            .map(|v| {
                (
                    v.qualifier("location").flatten(),
                    v.kind.clone(),
                    v.name.clone(),
                )
            })
            .collect()
    }

    fn at(location: Option<u32>, kind: &str, name: &str) -> Read {
        (location, kind.to_string(), name.to_string())
    }

    fn uniform(kind: &str, name: &str) -> (String, String) {
        (kind.to_string(), name.to_string())
    }

    #[test]
    fn the_glsl_declares_exactly_the_interface() {
        for &program in Program::ALL {
            let interface = program.interface();
            let name = program.name();
            assert_eq!(
                interface.check(interface.vertex, interface.fragment),
                Ok(()),
                "{name}"
            );
            // The check takes sources that leave something out; the
            // renderer's own declare it all.
            let vertex = declarations(interface.vertex);
            let fragment = declarations(interface.fragment);
            let inputs = read(&vertex.inputs);
            for attribute in interface.instance.iter().chain(interface.vertex_attributes) {
                let declared = at(
                    Some(attribute.location),
                    glsl_type(attribute.format),
                    attribute.name,
                );
                assert!(inputs.contains(&declared), "{name} {declared:?}");
            }
            for block in interface.uniform_blocks {
                let declared = block.name.to_string();
                assert!(
                    vertex.blocks.contains(&declared) || fragment.blocks.contains(&declared),
                    "{name} {declared}"
                );
            }
            for texture in interface.textures {
                let declared = uniform("sampler2D", texture);
                assert!(
                    vertex.uniforms.contains(&declared) || fragment.uniforms.contains(&declared),
                    "{name} {texture}"
                );
            }
        }
    }

    #[test]
    fn the_shader_folder_holds_the_sources_compiled_in() {
        for &program in Program::ALL {
            let interface = program.interface();
            let read = |file: &str| {
                fs::read_to_string(Path::new(SHADER_DIR).join(file)).expect("a shader file")
            };
            assert_eq!(read(program.vertex_file()), interface.vertex);
            assert_eq!(read(program.fragment_file()), interface.fragment);
            assert!(program.vertex_file().starts_with(program.name()));
        }
    }

    #[test]
    fn the_reader_finds_declarations_however_they_are_written() {
        let source = "#version 330 core
// layout(location = 9) in vec2 commented;
/* layout(location = 8) in vec2 also;
   uniform float commented_too; */
#define DECLARE(x) \\
    in vec2 x;
layout( location=3 )in vec4 uv_rect ;
layout(location = 12) in highp float a;
in vec2 unplaced;
attribute vec2 old;
layout(location = N) in vec2 by_macro;
layout(LOCATION = 0x1A) in vec2 hex;
layout(location = 010) in vec2 octal;
layout(location = 2, component = 2) in vec2 split;
flat out vec4 v_color;
out Varyings { vec2 uv; } vary;
layout(std140) uniform Camera { vec4 view; } camera;
uniform sampler2D first, second[2];
uniform float strength = 1.0, unset;
uniform vec4 tint = vec4(1.0, 0.5, 0.5, 1.0), weights[2] = vec4[2](vec4(1.0), vec4(0.0));
uniform struct Fx { float strength; } fx;
struct Light { vec2 at; float radius; };
vec2 helper(vec2 a) { return a; }
void main() { in_body(); { gl_Position = vec4(0.0); } }
";
        let found = declarations(source);
        assert_eq!(
            read(&found.inputs),
            [
                at(Some(3), "vec4", "uv_rect"),
                at(Some(12), "float", "a"),
                at(None, "vec2", "unplaced"),
                at(None, "vec2", "old"),
                at(None, "vec2", "by_macro"),
                at(Some(26), "vec2", "hex"),
                at(Some(8), "vec2", "octal"),
                at(Some(2), "vec2", "split"),
            ]
        );
        assert_eq!(
            found.inputs[7].declaration("in"),
            "layout(location = 2, component = 2) in vec2 split;"
        );
        assert_eq!(read(&found.outputs), [at(None, "vec4", "v_color")]);
        assert_eq!(found.blocks, ["Camera"]);
        assert_eq!(
            found.uniforms,
            [
                uniform("sampler2D", "first"),
                uniform("sampler2D", "second[2]"),
                uniform("float", "unset"),
                uniform("struct", "Fx {…}"),
            ]
        );
        assert!(found.built_in_outputs.is_empty());
        assert!(found.readable);
    }

    #[test]
    fn the_lines_are_read_as_the_preprocessor_reads_them() {
        let inputs = |source: &str| read(&declarations(source).inputs);
        let corner = |location| vec![at(Some(location), "vec2", "corner")];
        // A `\` before a newline joins the lines anywhere: a comment's line
        // takes the next, and a word is made whole.
        assert_eq!(
            inputs(
                "// \\\nlayout(location = 6) in vec2 corner;\nlay\\\nout(location = 7) in vec2 corner;"
            ),
            corner(7)
        );
        // A `\` before a space joins nothing, and one join never makes
        // another: the `\` before the `\` and its `\r\n` stays.
        assert_eq!(
            inputs("#define NOTHING \\ \nlayout(location = 7) in vec2 corner;"),
            corner(7)
        );
        assert_eq!(
            inputs("// path \\\\\r\n\nlayout(location = 7) in vec2 corner;"),
            corner(7)
        );
        // `\n\r` is one newline, joined by a `\` as `\r\n` is.
        assert_eq!(
            inputs(
                "#define OPEN \\\n\r{\nlayout(location = 7) in vec2 corner;\n#define CLOSE \\\n\r}"
            ),
            corner(7)
        );
        assert!(declarations("#define OPEN \\\n\r{\nvoid f() {}\n").readable);
        // A comment opened in a `#define` ends the directive where it ends;
        // on a `#pragma`, `#error` or `#extension` line it is the line's
        // own text.
        assert_eq!(
            inputs(
                "#define NOTHING /*\nlayout(location = 6) in vec2 corner;\n*/\nlayout(location = 7) in vec2 corner;"
            ),
            corner(7)
        );
        for directive in ["#pragma", "#error", "  #  extension", "# /*\n*/ pragma"] {
            assert_eq!(
                inputs(&format!(
                    "{directive} /*\nlayout(location = 7) in vec2 corner;\n{directive} */"
                )),
                corner(7),
                "{directive}"
            );
        }
        // Nor on the line of a directive Mesa does not know, and a string
        // holds no comment; but a comment on a `#line` is one.
        assert_eq!(
            inputs("#if 0\n#errorX /*\n#endif\nlayout(location = 7) in vec2 corner;\n// */"),
            corner(7)
        );
        assert_eq!(
            inputs("#define S \"/*\"\nlayout(location = 7) in vec2 corner;\n// */"),
            corner(7)
        );
        assert_eq!(
            inputs(
                "#line 20 /*\nlayout(location = 6) in vec2 corner;\n*/\nlayout(location = 7) in vec2 corner;"
            ),
            corner(7)
        );
        // A `#` alone is a directive of its own, and the next line is not
        // its name.
        assert_eq!(
            inputs("#\nerror e; layout(location = 7) in vec2 corner;"),
            corner(7)
        );
        // A comment before a `#` leaves it first on its line, and a line
        // comment after code leaves the next line a line of its own.
        assert_eq!(
            inputs("/* a\nb */ #define X\nlayout(location = 7) in vec2 corner;"),
            corner(7)
        );
        assert_eq!(
            inputs("in vec2 a; // c\n#define X\nlayout(location = 7) in vec2 corner;"),
            [at(None, "vec2", "a"), at(Some(7), "vec2", "corner")]
        );
        // A `\r` alone, or a `\r\n`, is a newline, and a vertical tab a
        // space.
        assert_eq!(
            inputs("// note\rlayout(location = 7) in vec2 corner;"),
            corner(7)
        );
        assert_eq!(
            inputs("#define X\r\nlayout(location = 7)\x0Bin vec2 corner;"),
            corner(7)
        );
        // An unterminated comment, or a directive on the last line, runs
        // to the end.
        assert_eq!(
            inputs("layout(location = 7) in vec2 corner; /* in vec2 x;"),
            corner(7)
        );
        assert_eq!(
            inputs("layout(location = 7) in vec2 corner;\n#define B"),
            corner(7)
        );
        // A `#` that is not first on its line is a mark.
        assert_eq!(
            tokens("a # b"),
            [Token::Word("a"), Token::Mark('#'), Token::Word("b")]
        );
        assert_eq!(tokens("é;"), [Token::Mark('é'), Token::Mark(';')]);
        assert_eq!(
            tokens("#ifdef A\n# elif B\n#else\n#endif\n#define C"),
            [
                Token::Branch(Branch::If),
                Token::Branch(Branch::Else),
                Token::Branch(Branch::Else),
                Token::Branch(Branch::End),
            ]
        );
    }

    #[test]
    fn integers_are_read_as_glsl_writes_them() {
        assert_eq!(integer("6"), Some(6));
        assert_eq!(integer("6u"), Some(6));
        assert_eq!(integer("0"), Some(0));
        assert_eq!(integer("010"), Some(8));
        assert_eq!(integer("0x1f"), Some(31));
        assert_eq!(integer("0XAU"), Some(10));
        assert_eq!(integer("08"), None);
        assert_eq!(integer("0x"), None);
        assert_eq!(integer("N"), None);
    }

    #[test]
    fn branches_and_braces_that_do_not_hold_whole_declarations_are_refused() {
        let readable = |source: &str| declarations(source).readable;
        // Each branch opens a brace, closed after the group: the uniform
        // after it is read.
        let paired =
            "#if 1\nvoid helper() {\n#else\nvoid helper2() {\n#endif\n}\nuniform float seen;";
        assert!(readable(paired));
        assert_eq!(declarations(paired).uniforms, [uniform("float", "seen")]);
        assert!(!readable("}"));
        assert!(!readable("void f() {"));
        // Braces paired across two groups of branches.
        assert!(!readable(
            "#if 0\nvoid hidden_a() {\n#endif\nuniform vec4 tint;\n#if 0\n}\n#endif"
        ));
        // A branch that splits a statement, or holds part of one.
        assert!(!readable("uniform\n#if 0\n;\n#endif\nvec4 tint;"));
        assert!(!readable("#if 0\nfoo\n#endif\nuniform vec4 tint;"));
        assert!(!readable(
            "#if 1\n#else\nfoo\n#endif\nlayout(location = 0) in vec2 corner;"
        ));
        // A group not closed, or not opened.
        assert!(!readable("#ifdef A\nin vec2 a;"));
        assert!(!readable("#endif"));
        assert!(!readable("#else"));
        // Whole declarations in each branch, and branches in a body that
        // each open a brace closed after them, are read.
        assert!(readable(
            "#ifdef A\nin vec2 a;\n#elif B\nin vec2 b;\n#else\n#endif\n"
        ));
        assert!(readable(
            "void main() {\n#ifdef A\n    if (a) {\n#else\n    if (b) {\n#endif\n    }\n}\nuniform float x;"
        ));
        assert!(readable(
            "#ifdef A\nvoid f() {\n#else\nvoid f() {\n#endif\n}"
        ));
        let hidden = "#if 0\nvoid hidden_a() {\n#endif\nuniform vec4 tint;\n#if 0\n}\n#endif";
        let fragment = format!("{}\n{hidden}", SPRITE.fragment);
        assert!(
            SPRITE
                .check(SPRITE.vertex, &fragment)
                .is_err_and(|e| e.starts_with("the fragment shader's braces, or the branches"))
        );
    }

    /// The sprite program's sources, with `from` replaced by `to`.
    fn sprite_with(from: &str, to: &str) -> Result<(), String> {
        let vertex = SPRITE.vertex.replace(from, to);
        let fragment = SPRITE.fragment.replace(from, to);
        assert!(
            vertex != SPRITE.vertex || fragment != SPRITE.fragment,
            "`{from}` is in the sources"
        );
        SPRITE.check(&vertex, &fragment)
    }

    #[test]
    fn an_attribute_moved_retyped_or_added_is_refused() {
        assert_eq!(
            sprite_with(
                "location = 6) in vec2 corner",
                "location = 7) in vec2 corner"
            ),
            Err(
                "`layout(location = 7) in vec2 corner;` is not one of the program's attributes \
                 (it has `layout(location = 6) in vec2 corner;`)"
                    .to_string()
            )
        );
        assert_eq!(
            sprite_with("in float rotation", "in vec2 rotation"),
            Err(
                "`layout(location = 5) in vec2 rotation;` is not one of the program's attributes \
                 (it has `layout(location = 5) in float rotation;`)"
                    .to_string()
            )
        );
        for added in ["in vec2 extra;", "attribute vec2 extra;"] {
            assert_eq!(
                sprite_with("out vec2 v_uv;", &format!("{added}\nout vec2 v_uv;")),
                Err(
                    "`in vec2 extra;` has no location, and is not one of the program's attributes"
                        .to_string()
                ),
                "{added}"
            );
        }
        // Read as GLSL reads it, `010` is location 8.
        assert!(
            sprite_with(
                "location = 6) in vec2 corner",
                "location = 010) in vec2 corner"
            )
            .is_err_and(|e| e.starts_with("`layout(location = 010) in vec2 corner;` is not"))
        );
        // Another layout qualifier binds it elsewhere.
        assert_eq!(
            sprite_with(
                "location = 6) in vec2 corner",
                "location = 6, component = 2) in vec2 corner"
            ),
            Err(
                "`layout(location = 6, component = 2) in vec2 corner;` is not one of the \
                 program's attributes (it has `layout(location = 6) in vec2 corner;`)"
                    .to_string()
            )
        );
        // In both branches of an `#if`, the difference is said once.
        assert_eq!(
            sprite_with(
                "layout(location = 6) in vec2 corner;",
                "#ifdef A\nlayout(location = 7) in vec2 corner;\n#else\nlayout(location = 7) \
                 in vec2 corner;\n#endif"
            ),
            Err(
                "`layout(location = 7) in vec2 corner;` is not one of the program's attributes \
                 (it has `layout(location = 6) in vec2 corner;`)"
                    .to_string()
            )
        );
    }

    #[test]
    fn a_uniform_the_renderer_does_not_set_is_refused() {
        assert_eq!(
            sprite_with("sprite_texture", "picture"),
            Err(
                "`uniform sampler2D picture;` is not one of the program's: the renderer sets \
                 only its uniform blocks and its samplers"
                    .to_string()
            )
        );
        assert_eq!(
            sprite_with(
                "uniform sampler2D sprite_texture;",
                "uniform isampler2D sprite_texture;"
            ),
            Err(
                "`uniform isampler2D sprite_texture;` is not one of the program's: the renderer \
                 sets only its uniform blocks and its samplers (`sprite_texture` is a `sampler2D`)"
                    .to_string()
            )
        );
        // In either shader.
        for shader in ["out vec2 v_uv;", "in vec2 v_uv;"] {
            let plain = sprite_with(shader, &format!("uniform float strength;\n{shader}"));
            assert!(
                plain.is_err_and(|e| e.starts_with("`uniform float strength;` is not one of")),
                "{shader}"
            );
            let block = sprite_with(
                shader,
                &format!("layout(std140) uniform Extra {{ vec4 e; }};\n{shader}"),
            );
            assert_eq!(
                block,
                Err(
                    "uniform block `Extra` is not one of the program's (it has `Camera`)"
                        .to_string()
                ),
                "{shader}"
            );
        }
        assert!(
            sprite_with(
                "in vec2 v_uv;",
                "uniform struct Fx { float strength; } fx;\nin vec2 v_uv;"
            )
            .is_err_and(|e| e.starts_with("`uniform struct Fx {…};` is not one of"))
        );
        assert!(
            sprite_with(
                "in vec2 v_uv;",
                "uniform struct { float strength; } fx;\nin vec2 v_uv;"
            )
            .is_err_and(|e| e.starts_with("`uniform struct {…};` is not one of"))
        );
        // A uniform after one with a value is still one without.
        assert!(
            sprite_with(
                "in vec2 v_uv;",
                "uniform vec2 offset = vec2(0.5, 0.5), scale;\nin vec2 v_uv;"
            )
            .is_err_and(|e| e.starts_with("`uniform vec2 scale;` is not one of"))
        );
        assert_eq!(
            sprite_with("uniform Camera", "uniform View"),
            Err("uniform block `View` is not one of the program's (it has `Camera`)".to_string())
        );
        assert_eq!(
            BLIT.check(
                &format!(
                    "{}\nlayout(std140) uniform Camera {{ vec4 view; }};",
                    BLIT.vertex
                ),
                BLIT.fragment
            ),
            Err("uniform block `Camera` is not one of the program's, which has none".to_string())
        );
    }

    #[test]
    fn the_fragment_shader_has_one_color_at_location_0() {
        for written in [
            "layout(location = 0) out vec4 out_color;",
            "layout(location = 0, index = 0) out vec4 out_color;",
            "#ifdef A\nout vec4 out_color;\n#else\nout vec4 out_color;\n#endif",
        ] {
            assert_eq!(
                sprite_with("out vec4 out_color;", written),
                Ok(()),
                "{written}"
            );
        }
        assert_eq!(
            sprite_with(
                "out vec4 out_color;",
                "layout(location = 1) out vec4 out_color;"
            ),
            Err(
                "the fragment shader has `layout(location = 1) out vec4 out_color;`, and must \
                 have one `out vec4`, at location 0: the renderer draws into one color"
                    .to_string()
            )
        );
        for (written, says) in [
            (
                "layout(location = 0, index = 1) out vec4 out_color;",
                "`layout(location = 0, index = 1) out vec4 out_color;`",
            ),
            (
                "layout(location = 0, INDEX = 0x1) out vec4 out_color;",
                "`layout(location = 0, INDEX = 0x1) out vec4 out_color;`",
            ),
            (
                "out vec4 debug;\nout vec4 out_color;",
                "`out vec4 debug;`, `out vec4 out_color;`",
            ),
            ("out vec3 out_color;", "`out vec3 out_color;`"),
            ("out vec4 out_color[2];", "`out vec4 out_color[2];`"),
        ] {
            assert!(
                sprite_with("out vec4 out_color;", written)
                    .is_err_and(|e| e.starts_with(&format!("the fragment shader has {says}, and"))),
                "{written}"
            );
        }
        assert_eq!(
            sprite_with("out vec4 out_color;", ""),
            Err(
                "the fragment shader has no output: it must have one `out vec4`, at location 0"
                    .to_string()
            )
        );
        for written in ["gl_FragData[1] = ", "gl_FragColor = "] {
            assert_eq!(
                sprite_with("out_color = ", written),
                Err(format!(
                    "the fragment shader writes `{}`: the renderer draws only its `out vec4`",
                    &written[..written.find([' ', '[']).unwrap_or(written.len())]
                )),
                "{written}"
            );
        }
    }

    #[test]
    fn an_edit_inside_the_interface_is_taken() {
        // A new body, a comment, a helper function and the layout written
        // another way change no declaration.
        assert_eq!(
            sprite_with(
                "out_color = texture(sprite_texture, v_uv) * v_color;",
                "out_color = tint(texture(sprite_texture, v_uv)) * v_color; // brighter"
            ),
            Ok(())
        );
        for written in [
            "layout (location=0)\nin vec2 position ;",
            "layout(LOCATION = 0x0) in highp vec2 position;",
            "layout(location = 0u) in mediump vec2 position;",
            "layout(location = 0) in lowp vec2 position;",
            "layout(location = 0) const in vec2 position;",
        ] {
            assert_eq!(
                sprite_with("layout(location = 0) in vec2 position;", written),
                Ok(()),
                "{written}"
            );
        }
        for (from, to) in [
            ("in vec4 v_color;", "flat in vec4 v_color;"),
            ("in vec4 v_color;", "smooth in vec4 v_color;"),
            (
                "in vec4 v_color;",
                "noperspective centroid in vec4 v_color;",
            ),
            ("out vec4 v_color;", "invariant out vec4 v_color;"),
        ] {
            assert_eq!(sprite_with(from, to), Ok(()), "{to}");
        }
        // The same declaration in both branches of an `#if`.
        assert_eq!(
            sprite_with(
                "layout(location = 6) in vec2 corner;",
                "#ifdef FAST\nlayout(location = 6) in vec2 corner;\n#else\nlayout(location = 6) \
                 in vec2 corner;\n#endif"
            ),
            Ok(())
        );
        // Varyings in a block of their own.
        let vertex = SPRITE
            .vertex
            .replace("out vec2 v_uv;", "out Varyings { vec2 uv; } vary;");
        let fragment = SPRITE
            .fragment
            .replace("in vec2 v_uv;", "in Varyings { vec2 uv; } vary;");
        assert_eq!(SPRITE.check(&vertex, &fragment), Ok(()));
        // A uniform with a value of its own needs none from the renderer.
        for written in [
            "uniform float strength = 1.5;",
            "uniform vec4 tint = vec4(1.0, 1.0, 1.0, 1.0);",
            "uniform float weights[2] = float[2](1.0, max(1.0, 2.0));",
        ] {
            assert_eq!(
                sprite_with("in vec2 v_uv;", &format!("{written}\nin vec2 v_uv;")),
                Ok(()),
                "{written}"
            );
        }
        // An attribute or a sampler left out is taken: the device enables
        // an attribute the program does not read, and warns of a sampler.
        assert_eq!(
            sprite_with("layout(location = 5) in float rotation;", ""),
            Ok(())
        );
        assert_eq!(sprite_with("uniform sampler2D sprite_texture;", ""), Ok(()));
        // The block and the sampler may be declared in either shader, or
        // in both.
        let fragment = format!(
            "{}\nlayout(std140) uniform Camera {{ vec4 view; }};",
            SPRITE.fragment
        );
        assert_eq!(SPRITE.check(SPRITE.vertex, &fragment), Ok(()));
    }

    #[test]
    fn the_reader_never_panics() {
        // Pieces of GLSL and of what breaks it, strung at random from a
        // fixed seed, then random bytes made text: the check reads edited
        // files, and must refuse, not panic.
        const PIECES: &[&str] = &[
            "layout",
            "(",
            ")",
            "location",
            "LOCATION",
            "index",
            "=",
            "0x1",
            "010",
            "6u",
            "in",
            "out",
            "attribute",
            "uniform",
            "struct",
            "flat",
            "{",
            "}",
            ";",
            ",",
            "[",
            "]",
            "#if",
            "#else",
            "#elif",
            "#endif",
            "#pragma",
            "#define",
            "#",
            "/*",
            "*/",
            "//",
            "\\",
            "\n",
            "\r",
            "\x0B",
            " ",
            "vec4",
            "vec2",
            "sampler2D",
            "x",
            "é",
            "gl_FragColor",
            "Camera",
        ];
        let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for round in 0..4_000 {
            let length = (next() % 64) as usize;
            let source: String = if round % 4 == 3 {
                let bytes: Vec<u8> = (0..length).map(|_| next() as u8).collect();
                String::from_utf8_lossy(&bytes).into_owned()
            } else {
                (0..length)
                    .map(|_| PIECES[(next() % PIECES.len() as u64) as usize])
                    .collect()
            };
            for interface in [&SPRITE, &TILE_CHUNK, &BLIT, &TEXT] {
                let _ = interface.check(&source, &source);
                let _ = interface.check(interface.vertex, &source);
            }
        }
    }

    #[test]
    fn the_sprite_instance_is_its_floats() {
        let (attributes, stride) = packed::<6>(SPRITE.instance);
        assert_eq!(stride, SPRITE_INSTANCE_FLOATS as u32 * 4);
        let offsets: Vec<u32> = attributes.iter().map(|a| a.offset / 4).collect();
        // The float each attribute starts at, as `push_instance` packs them.
        assert_eq!(offsets, [0, 2, 4, 6, 10, 14]);
    }

    #[test]
    fn the_text_program_takes_the_sprite_s_instances_and_vertices() {
        // The renderer packs a glyph as a sprite, into the same buffer.
        assert!(std::ptr::eq(TEXT.instance, SPRITE.instance));
        assert!(std::ptr::eq(
            TEXT.vertex_attributes,
            SPRITE.vertex_attributes
        ));
        assert_eq!(TEXT.uniform_blocks, SPRITE.uniform_blocks);
        // Its vertex shader is the sprite's, its comments aside.
        let code = |source: &str| -> Vec<String> {
            source
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty() && !line.starts_with("//"))
                .map(str::to_string)
                .collect()
        };
        assert_eq!(code(TEXT.vertex), code(SPRITE.vertex));
    }

    #[test]
    fn the_tile_instance_is_its_floats() {
        let (attributes, stride) = packed::<2>(TILE_CHUNK.instance);
        assert_eq!(stride, TILE_INSTANCE_FLOATS as u32 * 4);
        let offsets: Vec<u32> = attributes.iter().map(|a| a.offset / 4).collect();
        // The float each attribute starts at, as `TileLayer::pack_chunk`
        // packs them.
        assert_eq!(offsets, [0, 2]);
    }
}
