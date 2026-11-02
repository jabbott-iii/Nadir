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

//! The engine's components that are reflected by hand, [`Sprite`],
//! [`Collider`] and [`Animator`], the names of the game's collision layers
//! that colliders are written with, and the reflect contexts the engine
//! builds (ADR-0038, `docs/subsystems/scene.md`):
//!
//! - **`Sprite`** is an object of its fields: `texture` (its page's path),
//!   `uv`, `size`, `pivot`, `rotation`, `color`, `layer`, `sort` (`"YSort"`
//!   or `{"Fixed": n}`) and `flip` (`{"x": false, "y": false}`). Read, a
//!   sprite may instead name a frame of a cooked atlas, `{"atlas":
//!   "atlas/sprites.kbj", "sprite": "sprites/hero", "frame": 0}`, which
//!   loads the atlas and gives the frame's page, region, size and pivot, as
//!   [`Sprite::from_atlas`]; the other fields beside it apply over it. One
//!   needs a `texture` or an `atlas`, and a `sprite` or `frame` without its
//!   `atlas` is an error: a sprite keeps no atlas, so a layer that changes
//!   the frame names the atlas and the sprite again. Hashed ([`ReflectContext::hashing`]),
//!   it leaves out what `nadir::animate` writes: its texture, uv, size,
//!   pivot and `flip.x`.
//! - **`Collider`** is `{"shape": {"Aabb": {"half": [x, y]}}, "offset":
//!   [x, y], "layer": [...], "mask": [...], "kind": "Solid"}`, with the
//!   shape `{"Circle": {"radius": r}}` the other one. A layer mask is
//!   `"all"`, or a list of the game's layer names
//!   ([`App::collision_layers`](crate::App::collision_layers)) and bit
//!   numbers for the bits with no name. Read, a collider needs its `shape`;
//!   the rest default as [`Collider::solid`] has them.
//! - **`Animator`** is `{"clip": "sprites/hero/walk"}`, its clip's name in
//!   the [`Clips`]; its time and finished flag are left out, and a read one
//!   starts its clip.

use nadir_assets::{AssetServer, FinalizeCtx, Handle, Scope};
use nadir_collision::{ColliderKind, LayerMask, Shape};
use nadir_core::{Color, Vec2};
use nadir_ecs::World;
use nadir_render::{Atlas, Flip, SortMode, Texture};
use nadir_scene::{
    FieldInfo, FromValue, Reflect, ReflectContext, ReflectError, ReflectErrorKind, ReflectKind,
    Value,
};

use crate::{Animator, Clips, Collider, Sprite};

/// The names of the game's collision layers: the layer of bit `n` of a
/// [`LayerMask`] is named by the `n`-th, so that prefabs and levels write
/// `["World", "People"]` for a collider's layers. A resource of every
/// [`App`](crate::App), with no names until
/// [`App::collision_layers`](crate::App::collision_layers) gives them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CollisionLayers {
    names: Vec<String>,
}

impl CollisionLayers {
    /// Layers named `names`: bit 0 the first, bit 1 the next, and so on.
    ///
    /// # Panics
    ///
    /// If there are more than 32 names, one is empty, or two are the same:
    /// data could not tell them apart.
    pub fn new(names: &[&str]) -> Self {
        assert!(
            names.len() <= 32,
            "a layer mask has 32 bits, so there are at most 32 collision layers, not {}",
            names.len()
        );
        for (index, name) in names.iter().enumerate() {
            assert!(
                !name.is_empty(),
                "collision layer {index} has an empty name"
            );
            assert!(
                !names[..index].contains(name),
                "two collision layers are named `{name}`"
            );
        }
        Self {
            names: names.iter().map(|name| name.to_string()).collect(),
        }
    }

    /// The names, by bit.
    pub fn names(&self) -> &[String] {
        &self.names
    }

    /// The bit of the layer named `name`.
    pub fn bit(&self, name: &str) -> Option<u32> {
        self.names
            .iter()
            .position(|known| known == name)
            .and_then(|bit| u32::try_from(bit).ok())
    }

    /// The name of the layer of `bit`, if it has one.
    pub fn name(&self, bit: u32) -> Option<&str> {
        usize::try_from(bit)
            .ok()
            .and_then(|bit| self.names.get(bit))
            .map(String::as_str)
    }

    /// The mask of the layers named `names`, if each is a layer here.
    pub fn mask(&self, names: &[&str]) -> Option<LayerMask> {
        names.iter().try_fold(LayerMask::NONE, |mask, name| {
            self.bit(name)
                .map(|bit| LayerMask(mask.0 | LayerMask::bit(bit).0))
        })
    }
}

/// Runs `f` with a [`ReflectContext`] for `source` that has what the
/// engine's components need to be written: the [`AssetServer`], the
/// [`Clips`] and the [`CollisionLayers`] of `world`, each if `world` has
/// it. For writing values, such as a save's, or, with
/// [`ReflectContext::hashing`], a hash's, as [`world_hash`](crate::world_hash)
/// does; it never loads, so it cannot read a handle.
///
/// # Panics
///
/// If one of those resources is borrowed mutably elsewhere.
pub fn reflect_context<R>(
    world: &World,
    source: &str,
    f: impl FnOnce(ReflectContext<'_>) -> R,
) -> R {
    let server = world
        .contains_resource::<AssetServer>()
        .then(|| world.resource::<AssetServer>());
    let clips = world
        .contains_resource::<Clips>()
        .then(|| world.resource::<Clips>());
    let layers = world
        .contains_resource::<CollisionLayers>()
        .then(|| world.resource::<CollisionLayers>());
    let mut cx = ReflectContext::new(source);
    if let Some(server) = &server {
        cx = cx.with_assets(server);
    }
    if let Some(clips) = &clips {
        cx = cx.with_resource::<Clips>(clips);
    }
    if let Some(layers) = &layers {
        cx = cx.with_resource::<CollisionLayers>(layers);
    }
    f(cx)
}

/// Runs `f` with a [`ReflectContext`] for `source` that reads the engine's
/// components too: as [`reflect_context`], with the [`AssetServer`] to load
/// each handle read in `scope` ([`ReflectContext::loading`]). For spawning
/// prefabs and a level's entities
/// ([`PrefabLibrary`](nadir_scene::PrefabLibrary)).
///
/// # Panics
///
/// If `world` has no [`AssetServer`] (every [`App`](crate::App)'s has
/// one), or one of the resources is borrowed elsewhere.
pub fn loading_context<R>(
    world: &World,
    source: &str,
    scope: Scope,
    f: impl FnOnce(ReflectContext<'_>) -> R,
) -> R {
    let mut server = world.resource_mut::<AssetServer>();
    let clips = world
        .contains_resource::<Clips>()
        .then(|| world.resource::<Clips>());
    let layers = world
        .contains_resource::<CollisionLayers>()
        .then(|| world.resource::<CollisionLayers>());
    let mut cx = ReflectContext::new(source).loading(&mut server, scope);
    if let Some(clips) = &clips {
        cx = cx.with_resource::<Clips>(clips);
    }
    if let Some(layers) = &layers {
        cx = cx.with_resource::<CollisionLayers>(layers);
    }
    f(cx)
}

/// The parts of [`Reflect`] that are the same for the types here: each is
/// one value, read and written whole, with no fields to walk.
macro_rules! opaque {
    ($name:expr) => {
        fn type_name(&self) -> &'static str {
            $name
        }

        fn kind(&self) -> ReflectKind {
            ReflectKind::Primitive
        }

        fn fields(&self) -> &'static [FieldInfo] {
            &[]
        }

        fn field(&self, _index: usize) -> Option<&dyn Reflect> {
            None
        }

        fn field_mut(&mut self, _index: usize) -> Option<&mut dyn Reflect> {
            None
        }
    };
}

/// An object's members, or the error that it is not one.
fn object<'v>(
    value: &'v Value,
    cx: &ReflectContext<'_>,
    expected: &'static str,
) -> Result<&'v [(String, Value)], ReflectError> {
    value
        .as_object()
        .ok_or_else(|| cx.expected(expected, value))
}

/// The member `name` of `members`.
fn member<'v>(members: &'v [(String, Value)], name: &str) -> Option<&'v Value> {
    members
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value)
}

/// A member of an object, by its owned name.
fn pair(name: &str, value: Value) -> (String, Value) {
    (name.to_string(), value)
}

fn sort_to_value(sort: SortMode) -> Value {
    match sort {
        SortMode::YSort => Value::String("YSort".to_string()),
        SortMode::Fixed(depth) => Value::Object(vec![pair("Fixed", Value::Int(depth.into()))]),
    }
}

fn sort_from_value(value: &Value, cx: &mut ReflectContext<'_>) -> Result<SortMode, ReflectError> {
    const VARIANTS: &[&str] = &["YSort", "Fixed"];
    let unknown = |cx: &ReflectContext<'_>, found: &str| {
        cx.error(ReflectErrorKind::UnknownVariant {
            type_name: "SortMode",
            found: found.to_string(),
            variants: VARIANTS,
        })
    };
    match value {
        Value::String(name) if name == "YSort" => Ok(SortMode::YSort),
        Value::String(name) => Err(unknown(cx, name)),
        Value::Object(members) => match members.as_slice() {
            [(name, depth)] if name == "Fixed" => {
                let depth = cx.field("Fixed", |cx| u32::from_value(depth, cx))?;
                Ok(SortMode::Fixed(depth))
            }
            [(name, _)] => Err(unknown(cx, name)),
            _ => Err(cx.expected("`\"YSort\"` or `{\"Fixed\": depth}`", value)),
        },
        _ => Err(cx.expected("`\"YSort\"` or `{\"Fixed\": depth}`", value)),
    }
}

fn apply_flip(
    flip: &mut Flip,
    value: &Value,
    cx: &mut ReflectContext<'_>,
) -> Result<(), ReflectError> {
    for (key, member) in object(value, cx, "an object `{\"x\": false, \"y\": false}`")? {
        match key.as_str() {
            "x" => flip.x = cx.field("x", |cx| bool::from_value(member, cx))?,
            "y" => flip.y = cx.field("y", |cx| bool::from_value(member, cx))?,
            _ => cx.unknown_field("Flip", key),
        }
    }
    Ok(())
}

/// The sprite of frame `frame` of the sprite `name` of the atlas at
/// `path`, loaded with its pages through the context's server.
fn sprite_from_atlas(
    path: &Value,
    name: &Value,
    frame: usize,
    cx: &mut ReflectContext<'_>,
) -> Result<Sprite, ReflectError> {
    let path = cx.field("atlas", |cx| {
        path.as_str()
            .ok_or_else(|| cx.expected("an atlas file's path", path))
    })?;
    let name = cx.field("sprite", |cx| {
        name.as_str()
            .ok_or_else(|| cx.expected("a sprite's name in the atlas", name))
    })?;
    let Some((server, scope)) = cx.assets_mut() else {
        return Err(cx.error(ReflectErrorKind::NoContext {
            needs: "the asset server, to load an atlas",
        }));
    };
    let made = load_atlas_sprite(server, scope, path, name, frame);
    made.map_err(|kind| cx.error(kind))
}

/// [`sprite_from_atlas`]'s loads, which hold the server.
fn load_atlas_sprite(
    server: &mut AssetServer,
    scope: Scope,
    path: &str,
    name: &str,
    frame: usize,
) -> Result<Sprite, ReflectErrorKind> {
    let loaded = server
        .load_now_in::<Atlas>(path, scope, &mut FinalizeCtx::new())
        .map_err(|error| ReflectErrorKind::BadAssetPath {
            path: path.to_string(),
            reason: error.to_string(),
        })?;
    let atlas = server
        .get(loaded)
        .cloned()
        .ok_or_else(|| ReflectErrorKind::BadAssetPath {
            path: path.to_string(),
            reason: "the atlas did not load".to_string(),
        })?;
    let pages = atlas.load_pages(server, scope);
    let unknown = |what: &'static str, name: String| ReflectErrorKind::UnknownName { what, name };
    let sprite = atlas
        .sprite(name)
        .ok_or_else(|| unknown("sprite in the atlas", name.to_string()))?;
    let page = sprite
        .frame(frame)
        .and_then(|frame| pages.get(frame.page()).copied())
        .ok_or_else(|| unknown("frame of the sprite", format!("{name} {frame}")))?;
    Sprite::from_atlas(page, sprite, frame)
        .ok_or_else(|| unknown("sprite with a pivot, not a tile set,", name.to_string()))
}

impl Sprite {
    /// Applies the members of a sprite's value but its atlas form's.
    fn apply_members(
        &mut self,
        members: &[(String, Value)],
        cx: &mut ReflectContext<'_>,
    ) -> Result<(), ReflectError> {
        for (key, member) in members {
            match key.as_str() {
                "atlas" | "sprite" | "frame" => {}
                "texture" => {
                    self.texture =
                        cx.field("texture", |cx| Handle::<Texture>::from_value(member, cx))?;
                }
                "uv" => cx.field("uv", |cx| self.uv.apply_value(member, cx))?,
                "size" => self.size = cx.field("size", |cx| Vec2::from_value(member, cx))?,
                "pivot" => self.pivot = cx.field("pivot", |cx| Vec2::from_value(member, cx))?,
                "rotation" => {
                    self.rotation = cx.field("rotation", |cx| f32::from_value(member, cx))?;
                }
                "color" => self.color = cx.field("color", |cx| Color::from_value(member, cx))?,
                "layer" => self.layer = cx.field("layer", |cx| u8::from_value(member, cx))?,
                "sort" => self.sort = cx.field("sort", |cx| sort_from_value(member, cx))?,
                "flip" => cx.field("flip", |cx| apply_flip(&mut self.flip, member, cx))?,
                _ => cx.unknown_field("Sprite", key),
            }
        }
        Ok(())
    }

    /// The sprite of its atlas form's members, if it has them.
    fn atlas_form(
        members: &[(String, Value)],
        cx: &mut ReflectContext<'_>,
    ) -> Result<Option<Sprite>, ReflectError> {
        let Some(atlas) = member(members, "atlas") else {
            // A frame or a sprite's name alone cannot be applied: a sprite
            // keeps no atlas to look them up in.
            if member(members, "sprite").is_some() || member(members, "frame").is_some() {
                return Err(cx.error(ReflectErrorKind::MissingField { field: "atlas" }));
            }
            return Ok(None);
        };
        let name = member(members, "sprite")
            .ok_or_else(|| cx.error(ReflectErrorKind::MissingField { field: "sprite" }))?;
        let frame = match member(members, "frame") {
            Some(frame) => cx.field("frame", |cx| usize::from_value(frame, cx))?,
            None => 0,
        };
        sprite_from_atlas(atlas, name, frame, cx).map(Some)
    }
}

impl Reflect for Sprite {
    opaque!("Sprite");

    fn to_value(&self, cx: &mut ReflectContext<'_>) -> Value {
        let mut members = Vec::with_capacity(9);
        if !cx.is_hashing() {
            let texture = cx.field("texture", |cx| self.texture.to_value(cx));
            members.push(pair("texture", texture));
            members.push(pair("uv", self.uv.to_value(cx)));
            members.push(pair("size", self.size.to_value(cx)));
            members.push(pair("pivot", self.pivot.to_value(cx)));
        }
        members.push(pair("rotation", self.rotation.to_value(cx)));
        members.push(pair("color", self.color.to_value(cx)));
        members.push(pair("layer", self.layer.to_value(cx)));
        members.push(pair("sort", sort_to_value(self.sort)));
        let mut flip = Vec::with_capacity(2);
        if !cx.is_hashing() {
            flip.push(pair("x", Value::Bool(self.flip.x)));
        }
        flip.push(pair("y", Value::Bool(self.flip.y)));
        members.push(pair("flip", Value::Object(flip)));
        Value::Object(members)
    }

    /// Sets the fields the value names; an atlas form sets the texture, uv,
    /// size and pivot, and the fields beside it apply over it.
    fn apply_value(
        &mut self,
        value: &Value,
        cx: &mut ReflectContext<'_>,
    ) -> Result<(), ReflectError> {
        let members = object(value, cx, "an object of a sprite's fields")?;
        if let Some(made) = Self::atlas_form(members, cx)? {
            self.texture = made.texture;
            self.uv = made.uv;
            self.size = made.size;
            self.pivot = made.pivot;
        }
        self.apply_members(members, cx)
    }
}

impl FromValue for Sprite {
    const TYPE_NAME: &'static str = "Sprite";

    /// A sprite of its atlas form, or of its `texture`, with all of it as
    /// its region and no size until the value gives one; then the other
    /// fields the value names.
    fn from_value(value: &Value, cx: &mut ReflectContext<'_>) -> Result<Self, ReflectError> {
        let members = object(value, cx, "an object of a sprite's fields")?;
        let mut sprite = match Self::atlas_form(members, cx)? {
            Some(sprite) => sprite,
            None => {
                let texture = member(members, "texture")
                    .ok_or_else(|| cx.error(ReflectErrorKind::MissingField { field: "texture" }))?;
                let texture =
                    cx.field("texture", |cx| Handle::<Texture>::from_value(texture, cx))?;
                Sprite::new(texture, Vec2::ZERO)
            }
        };
        sprite.apply_members(members, cx)?;
        Ok(sprite)
    }
}

fn shape_to_value(shape: Shape, cx: &mut ReflectContext<'_>) -> Value {
    let (name, field, value) = match shape {
        Shape::Aabb { half } => ("Aabb", "half", half.to_value(cx)),
        Shape::Circle { radius } => ("Circle", "radius", radius.to_value(cx)),
    };
    Value::Object(vec![pair(name, Value::Object(vec![pair(field, value)]))])
}

fn shape_from_value(value: &Value, cx: &mut ReflectContext<'_>) -> Result<Shape, ReflectError> {
    const VARIANTS: &[&str] = &["Aabb", "Circle"];
    let expected = "a shape `{\"Aabb\": {\"half\": [x, y]}}` or `{\"Circle\": {\"radius\": r}}`";
    let [(name, fields)] = object(value, cx, expected)? else {
        return Err(cx.expected(expected, value));
    };
    let fields = cx.key(name, |cx| {
        object(fields, cx, "an object of the shape's fields")
    })?;
    let field = |cx: &mut ReflectContext<'_>, field: &'static str| {
        member(fields, field).ok_or_else(|| {
            cx.key(name, |cx| {
                cx.error(ReflectErrorKind::MissingField { field })
            })
        })
    };
    // Warns of the members a shape has not got, as every reader does.
    let others = |cx: &mut ReflectContext<'_>, own: &str| {
        cx.key(name, |cx| {
            for (key, _) in fields {
                if key != own {
                    cx.unknown_field("Shape", key);
                }
            }
        });
    };
    match name.as_str() {
        "Aabb" => {
            let half = field(cx, "half")?;
            let half = cx.key(name, |cx| cx.field("half", |cx| Vec2::from_value(half, cx)))?;
            others(cx, "half");
            Ok(Shape::Aabb { half })
        }
        "Circle" => {
            let radius = field(cx, "radius")?;
            let radius = cx.key(name, |cx| {
                cx.field("radius", |cx| f32::from_value(radius, cx))
            })?;
            others(cx, "radius");
            Ok(Shape::Circle { radius })
        }
        _ => Err(cx.error(ReflectErrorKind::UnknownVariant {
            type_name: "Shape",
            found: name.clone(),
            variants: VARIANTS,
        })),
    }
}

fn mask_to_value(mask: LayerMask, cx: &ReflectContext<'_>) -> Value {
    if mask == LayerMask::ALL {
        return Value::String("all".to_string());
    }
    let layers = cx.resource::<CollisionLayers>();
    let bits = (0..32).filter(|bit| mask.0 & (1 << bit) != 0);
    Value::Array(
        bits.map(|bit| match layers.and_then(|layers| layers.name(bit)) {
            Some(name) => Value::String(name.to_string()),
            None => Value::Int(bit.into()),
        })
        .collect(),
    )
}

fn mask_from_value(value: &Value, cx: &mut ReflectContext<'_>) -> Result<LayerMask, ReflectError> {
    let expected = "`\"all\"`, or a list of collision layers' names and bit numbers";
    if value.as_str() == Some("all") {
        return Ok(LayerMask::ALL);
    }
    let items = value
        .as_array()
        .ok_or_else(|| cx.expected(expected, value))?;
    let mut mask = LayerMask::NONE;
    for (index, item) in items.iter().enumerate() {
        let bit = cx.index(index, |cx| match item {
            Value::String(name) => {
                let Some(layers) = cx.resource::<CollisionLayers>() else {
                    return Err(cx.error(ReflectErrorKind::NoContext {
                        needs: "the game's collision layer names (`App::collision_layers`)",
                    }));
                };
                layers.bit(name).ok_or_else(|| {
                    cx.error(ReflectErrorKind::UnknownName {
                        what: "collision layer",
                        name: name.clone(),
                    })
                })
            }
            Value::Int(bit) if (0..32).contains(bit) => {
                Ok(u32::try_from(*bit).expect("0 to 31 fits"))
            }
            Value::Int(bit) => Err(cx.error(ReflectErrorKind::OutOfRange {
                type_name: "a collision layer's bit, 0 to 31,",
                value: bit.to_string(),
            })),
            _ => Err(cx.expected("a collision layer's name or bit number", item)),
        })?;
        mask = LayerMask(mask.0 | LayerMask::bit(bit).0);
    }
    Ok(mask)
}

fn kind_from_value(value: &Value, cx: &ReflectContext<'_>) -> Result<ColliderKind, ReflectError> {
    match value.as_str() {
        Some("Solid") => Ok(ColliderKind::Solid),
        Some("Trigger") => Ok(ColliderKind::Trigger),
        Some(found) => Err(cx.error(ReflectErrorKind::UnknownVariant {
            type_name: "ColliderKind",
            found: found.to_string(),
            variants: &["Solid", "Trigger"],
        })),
        None => Err(cx.expected("`\"Solid\"` or `\"Trigger\"`", value)),
    }
}

impl Reflect for Collider {
    opaque!("Collider");

    fn to_value(&self, cx: &mut ReflectContext<'_>) -> Value {
        let collider = self.0;
        let kind = match collider.kind {
            ColliderKind::Solid => "Solid",
            ColliderKind::Trigger => "Trigger",
        };
        Value::Object(vec![
            pair("shape", shape_to_value(collider.shape, cx)),
            pair("offset", collider.offset.to_value(cx)),
            pair("layer", mask_to_value(collider.layer, cx)),
            pair("mask", mask_to_value(collider.mask, cx)),
            pair("kind", Value::String(kind.to_string())),
        ])
    }

    fn apply_value(
        &mut self,
        value: &Value,
        cx: &mut ReflectContext<'_>,
    ) -> Result<(), ReflectError> {
        let members = object(value, cx, "an object of a collider's fields")?;
        self.apply_members(members, true, cx)
    }
}

impl Collider {
    /// Applies the members of a collider's value: its shape too, if
    /// `shape`.
    fn apply_members(
        &mut self,
        members: &[(String, Value)],
        shape: bool,
        cx: &mut ReflectContext<'_>,
    ) -> Result<(), ReflectError> {
        for (key, member) in members {
            let collider = &mut self.0;
            match key.as_str() {
                "shape" if !shape => {}
                "shape" => collider.shape = cx.field("shape", |cx| shape_from_value(member, cx))?,
                "offset" => {
                    collider.offset = cx.field("offset", |cx| Vec2::from_value(member, cx))?;
                }
                "layer" => collider.layer = cx.field("layer", |cx| mask_from_value(member, cx))?,
                "mask" => collider.mask = cx.field("mask", |cx| mask_from_value(member, cx))?,
                "kind" => collider.kind = cx.field("kind", |cx| kind_from_value(member, cx))?,
                _ => cx.unknown_field("Collider", key),
            }
        }
        Ok(())
    }
}

impl FromValue for Collider {
    const TYPE_NAME: &'static str = "Collider";

    /// A solid collider on every layer, colliding with every layer, at the
    /// entity's position, as [`Collider::solid`] makes one, of the value's
    /// `shape`, which it needs; then the other fields the value names.
    fn from_value(value: &Value, cx: &mut ReflectContext<'_>) -> Result<Self, ReflectError> {
        let members = object(value, cx, "an object of a collider's fields")?;
        let shape = member(members, "shape")
            .ok_or_else(|| cx.error(ReflectErrorKind::MissingField { field: "shape" }))?;
        let shape = cx.field("shape", |cx| shape_from_value(shape, cx))?;
        let mut collider = Collider::solid(shape);
        collider.apply_members(members, false, cx)?;
        Ok(collider)
    }
}

impl Reflect for Animator {
    opaque!("Animator");

    /// Its clip's name. A clip that has none, or a context with no
    /// [`Clips`], is written as `null`, with a warning; for a hash, as its
    /// handle's index and generation, which tell clips apart and are the
    /// same on every run, with no warning.
    fn to_value(&self, cx: &mut ReflectContext<'_>) -> Value {
        let name = cx
            .resource::<Clips>()
            .and_then(|clips| clips.name(self.clip()))
            .map(str::to_string);
        let clip = match name {
            Some(name) => Value::String(name),
            None if cx.is_hashing() => {
                let id = self.clip().id();
                Value::Array(vec![
                    Value::Int(id.index().into()),
                    Value::Int(id.generation().get().into()),
                ])
            }
            None => {
                cx.field("clip", |cx| {
                    cx.warn("an animator's clip with no name in the clips is written as null")
                });
                Value::Null
            }
        };
        Value::Object(vec![pair("clip", clip)])
    }

    /// Plays the clip the value names, from its start.
    fn apply_value(
        &mut self,
        value: &Value,
        cx: &mut ReflectContext<'_>,
    ) -> Result<(), ReflectError> {
        *self = Self::from_value(value, cx)?;
        Ok(())
    }
}

impl FromValue for Animator {
    const TYPE_NAME: &'static str = "Animator";

    fn from_value(value: &Value, cx: &mut ReflectContext<'_>) -> Result<Self, ReflectError> {
        let members = object(value, cx, "an object `{\"clip\": name}`")?;
        for (key, _) in members {
            if key != "clip" {
                cx.unknown_field("Animator", key);
            }
        }
        let clip = member(members, "clip")
            .ok_or_else(|| cx.error(ReflectErrorKind::MissingField { field: "clip" }))?;
        cx.field("clip", |cx| {
            let name = clip
                .as_str()
                .ok_or_else(|| cx.expected("a clip's name", clip))?;
            let Some(clips) = cx.resource::<Clips>() else {
                return Err(cx.error(ReflectErrorKind::NoContext {
                    needs: "the clips (`Clips`), to find a clip by its name",
                }));
            };
            let handle = clips.find(name).ok_or_else(|| {
                cx.error(ReflectErrorKind::UnknownName {
                    what: "clip",
                    name: name.to_string(),
                })
            })?;
            Ok(Animator::new(handle))
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::time::Duration;

    use nadir_assets::{AssetError, AssetPath, OnFailure, Source};
    use nadir_codec::{Limits, json, kbj};
    use nadir_core::Rect;

    use super::*;
    use crate::{Clip, ClipFrame, Playback};

    /// Files in memory, by path.
    struct Memory(HashMap<String, Vec<u8>>);

    impl Source for Memory {
        fn read(&self, path: &AssetPath) -> Result<Vec<u8>, AssetError> {
            self.0
                .get(path.as_str())
                .cloned()
                .ok_or(AssetError::NotFound)
        }
    }

    /// A server of one cooked atlas, `atlas/sprites.kbj`: the hero, two
    /// frames of 12 × 20 on a page of 64, the first trimmed by a pixel.
    fn server() -> AssetServer {
        let atlas = r#"{ "version": 2,
            "pages": [ { "path": "atlas/sprites_0.qoi", "w": 64, "h": 64 } ],
            "sprites": [ { "name": "sprites/hero",
                "frames": [ { "page": 0, "x": 0, "y": 0, "w": 11, "h": 20, "ox": 1, "oy": 0, "sw": 12, "sh": 20 },
                            { "page": 0, "x": 12, "y": 0, "w": 12, "h": 20, "ox": 0, "oy": 0, "sw": 12, "sh": 20 } ],
                "pivot": [6.0, 20.0] } ] }"#;
        let value = json::parse(atlas, Limits::default()).expect("JSON");
        let bytes = kbj::encode(&value, Limits::default()).expect("KBJ");
        let files = HashMap::from([("atlas/sprites.kbj".to_string(), bytes)]);
        AssetServer::with_source(Memory(files), OnFailure::Nothing)
    }

    fn value(text: &str) -> Value {
        json::parse(text, Limits::default()).expect("the test's JSON")
    }

    #[test]
    fn a_sprite_of_an_atlas_frame_writes_back_its_fields_and_reads_again() {
        let mut server = server();
        let mut cx = ReflectContext::new("test").loading(&mut server, Scope::Level);
        let form = value(
            r#"{ "atlas": "atlas/sprites.kbj", "sprite": "sprites/hero", "frame": 1,
                 "layer": 1, "sort": { "Fixed": 3 }, "flip": { "x": true } }"#,
        );
        let sprite = Sprite::from_value(&form, &mut cx).expect("a sprite");
        assert_eq!(sprite.size, Vec2::new(12.0, 20.0));
        assert_eq!(sprite.layer, 1);
        assert_eq!(sprite.sort, SortMode::Fixed(3));
        assert!(sprite.flip.x && !sprite.flip.y);
        assert!(
            (sprite.uv.min.x - 12.0 / 64.0).abs() < 1e-6,
            "{:?}",
            sprite.uv
        );
        // Written back whole, with the page's path, and read to the same.
        let written = sprite.to_value(&mut cx);
        assert_eq!(
            written.get("texture"),
            Some(&Value::String("atlas/sprites_0.qoi".into()))
        );
        let again = Sprite::from_value(&written, &mut cx).expect("the written sprite");
        assert_eq!(again, sprite);
        drop(cx);
        // Hashed, it leaves out what animators write.
        let mut cx = ReflectContext::new("test").with_assets(&server).hashing();
        let hashed = sprite.to_value(&mut cx);
        for left_out in ["texture", "uv", "size", "pivot"] {
            assert_eq!(hashed.get(left_out), None, "{left_out}");
        }
        assert_eq!(
            json::to_string_compact(hashed.get("flip").expect("flip")),
            r#"{"y":false}"#
        );
        assert_eq!(hashed.get("layer"), Some(&Value::Int(1)));
    }

    #[test]
    fn a_sprite_needs_a_texture_or_an_atlas_and_names_what_is_wrong() {
        let mut server = server();
        let mut cx = ReflectContext::new("prefab").loading(&mut server, Scope::Level);
        let error = Sprite::from_value(&value(r#"{ "layer": 1 }"#), &mut cx).expect_err("none");
        assert_eq!(
            error.kind,
            ReflectErrorKind::MissingField { field: "texture" }
        );
        let error = Sprite::from_value(
            &value(r#"{ "atlas": "atlas/sprites.kbj", "sprite": "sprites/villain" }"#),
            &mut cx,
        )
        .expect_err("no such sprite");
        assert!(
            matches!(&error.kind, ReflectErrorKind::UnknownName { name, .. } if name == "sprites/villain"),
            "{error}"
        );
        let error = Sprite::from_value(
            &value(r#"{ "texture": "atlas/sprites_0.qoi", "sort": "Sideways" }"#),
            &mut cx,
        )
        .expect_err("no such sort");
        assert_eq!(error.path, "sort");
        // Without a server to load with, a sprite cannot be read.
        drop(cx);
        let mut cx = ReflectContext::new("prefab");
        let error = Sprite::from_value(&value(r#"{ "texture": "a.qoi" }"#), &mut cx)
            .expect_err("no server");
        assert!(
            matches!(error.kind, ReflectErrorKind::NoContext { .. }),
            "{error}"
        );
    }

    #[test]
    fn a_collider_writes_its_layers_by_name_and_reads_them_back() {
        let layers = CollisionLayers::new(&["World", "People"]);
        let mut cx = ReflectContext::new("test").with_resource(&layers);
        let read = Collider::from_value(
            &value(
                r#"{ "shape": { "Circle": { "radius": 6 } }, "offset": [0, -3],
                     "layer": ["People"], "mask": ["World", 5], "kind": "Trigger" }"#,
            ),
            &mut cx,
        )
        .expect("a collider");
        assert_eq!(read.0.shape, Shape::Circle { radius: 6.0 });
        assert_eq!(read.0.offset, Vec2::new(0.0, -3.0));
        assert_eq!(read.0.layer, LayerMask::bit(1));
        assert_eq!(
            read.0.mask,
            LayerMask(LayerMask::bit(0).0 | LayerMask::bit(5).0)
        );
        assert_eq!(read.0.kind, ColliderKind::Trigger);
        let written = read.to_value(&mut cx);
        assert_eq!(
            json::to_string_compact(&written),
            r#"{"shape":{"Circle":{"radius":6.0}},"offset":[0.0,-3.0],"layer":["People"],"mask":["World",5],"kind":"Trigger"}"#
        );
        assert_eq!(
            Collider::from_value(&written, &mut cx).expect("again"),
            read
        );
        // The defaults are a solid's, on and against every layer.
        let solid = Collider::from_value(
            &value(r#"{ "shape": { "Aabb": { "half": [5, 3] } } }"#),
            &mut cx,
        )
        .expect("a box");
        assert_eq!(
            solid,
            Collider::solid(Shape::Aabb {
                half: Vec2::new(5.0, 3.0)
            })
        );
        assert_eq!(
            json::to_string_compact(solid.to_value(&mut cx).get("layer").expect("layer")),
            r#""all""#
        );
        assert_eq!(layers.mask(&["World", "People"]), Some(LayerMask(0b11)));
    }

    #[test]
    fn a_collider_s_mistakes_are_named() {
        let layers = CollisionLayers::new(&["World"]);
        let mut cx = ReflectContext::new("prefab").with_resource(&layers);
        let cases = [
            (
                r#"{}"#,
                "",
                ReflectErrorKind::MissingField { field: "shape" },
            ),
            (
                r#"{ "shape": { "Circle": {} } }"#,
                "shape.Circle",
                ReflectErrorKind::MissingField { field: "radius" },
            ),
            (
                r#"{ "shape": { "Circle": { "radius": 1 } }, "mask": ["Enemy"] }"#,
                "mask[0]",
                ReflectErrorKind::UnknownName {
                    what: "collision layer",
                    name: "Enemy".into(),
                },
            ),
        ];
        for (text, path, kind) in cases {
            let error = Collider::from_value(&value(text), &mut cx).expect_err(text);
            assert_eq!((error.path.as_str(), error.kind), (path, kind), "{text}");
        }
        let error = Collider::from_value(
            &value(r#"{ "shape": { "Square": { "side": 1 } } }"#),
            &mut cx,
        )
        .expect_err("no such shape");
        assert!(
            matches!(error.kind, ReflectErrorKind::UnknownVariant { .. }),
            "{error}"
        );
        let error = Collider::from_value(
            &value(r#"{ "shape": { "Circle": { "radius": 1 } }, "layer": [32] }"#),
            &mut cx,
        )
        .expect_err("a bit past 31");
        assert_eq!(error.path, "layer[0]");
        // Names need the game's layers.
        let mut cx = ReflectContext::new("prefab");
        let error = Collider::from_value(
            &value(r#"{ "shape": { "Circle": { "radius": 1 } }, "layer": ["World"] }"#),
            &mut cx,
        )
        .expect_err("no layers");
        assert!(
            matches!(error.kind, ReflectErrorKind::NoContext { .. }),
            "{error}"
        );
    }

    #[test]
    #[should_panic(expected = "two collision layers are named `World`")]
    fn two_layers_of_one_name_are_refused() {
        let _ = CollisionLayers::new(&["World", "World"]);
    }

    #[test]
    fn an_animator_is_its_clip_s_name() {
        let mut clips = Clips::new();
        let frame = |ms| {
            ClipFrame::new(
                Handle::new(nadir_core::GenIndex::new(0, std::num::NonZeroU32::MIN)),
                Rect::new(Vec2::ZERO, Vec2::ONE),
                Duration::from_millis(ms),
            )
        };
        let walk = clips
            .add_named(
                "sprites/hero/walk",
                Clip::new(vec![frame(100)], Playback::Loop),
            )
            .expect("a new name");
        let nameless = clips.add(Clip::new(vec![frame(50)], Playback::Once));
        let mut cx = ReflectContext::new("test").with_resource(&clips);
        let read = Animator::from_value(&value(r#"{ "clip": "sprites/hero/walk" }"#), &mut cx)
            .expect("an animator");
        assert_eq!(read.clip(), walk);
        assert_eq!(read.elapsed(), Duration::ZERO, "it starts its clip");
        assert_eq!(
            json::to_string_compact(&read.to_value(&mut cx)),
            r#"{"clip":"sprites/hero/walk"}"#
        );
        // One whose clip has no name is written with a null clip.
        assert_eq!(
            json::to_string_compact(&Animator::new(nameless).to_value(&mut cx)),
            r#"{"clip":null}"#
        );
        let error = Animator::from_value(&value(r#"{ "clip": "sprites/hero/fly" }"#), &mut cx)
            .expect_err("no such clip");
        assert_eq!(error.path, "clip");
        assert!(
            matches!(error.kind, ReflectErrorKind::UnknownName { .. }),
            "{error}"
        );
        let mut cx = ReflectContext::new("test");
        let error = Animator::from_value(&value(r#"{ "clip": "sprites/hero/walk" }"#), &mut cx)
            .expect_err("no clips");
        assert!(
            matches!(error.kind, ReflectErrorKind::NoContext { .. }),
            "{error}"
        );
    }

    #[test]
    fn a_frame_without_its_atlas_and_a_shape_s_stray_member_are_not_dropped_in_silence() {
        let mut server = server();
        let mut cx = ReflectContext::new("prefab").loading(&mut server, Scope::Level);
        let mut sprite = Sprite::from_value(
            &value(r#"{ "atlas": "atlas/sprites.kbj", "sprite": "sprites/hero" }"#),
            &mut cx,
        )
        .expect("a sprite");
        // A derived layer that changes the frame alone cannot be applied.
        let error = sprite
            .apply_value(&value(r#"{ "frame": 1 }"#), &mut cx)
            .expect_err("a frame of no atlas");
        assert_eq!(
            error.kind,
            ReflectErrorKind::MissingField { field: "atlas" }
        );
        let error = Sprite::from_value(
            &value(r#"{ "texture": "atlas/sprites_0.qoi", "sprite": "sprites/hero" }"#),
            &mut cx,
        )
        .expect_err("a sprite of no atlas");
        assert_eq!(
            error.kind,
            ReflectErrorKind::MissingField { field: "atlas" }
        );
        // Naming them again applies.
        sprite
            .apply_value(
                &value(r#"{ "atlas": "atlas/sprites.kbj", "sprite": "sprites/hero", "frame": 1 }"#),
                &mut cx,
            )
            .expect("the second frame");
        assert_eq!(sprite.size, Vec2::new(12.0, 20.0));
        drop(cx);

        let mut cx = ReflectContext::new("prefab");
        Collider::from_value(
            &value(r#"{ "shape": { "Circle": { "radius": 6, "offset": [0, -3] } } }"#),
            &mut cx,
        )
        .expect("a circle");
        let warnings: Vec<(&str, &str)> = cx
            .warnings()
            .iter()
            .map(|warning| (warning.field.as_str(), warning.path.as_str()))
            .collect();
        assert_eq!(warnings, [("offset", "shape.Circle")]);
    }

    #[test]
    fn a_nameless_clip_hashes_as_its_handle() {
        let mut clips = Clips::new();
        let frame = ClipFrame::new(
            Handle::new(nadir_core::GenIndex::new(0, std::num::NonZeroU32::MIN)),
            Rect::new(Vec2::ZERO, Vec2::ONE),
            Duration::from_millis(100),
        );
        let first = clips.add(Clip::new(vec![frame], Playback::Loop));
        let second = clips.add(Clip::new(vec![frame], Playback::Loop));
        let mut cx = ReflectContext::new("hash").with_resource(&clips).hashing();
        let a = Animator::new(first).to_value(&mut cx);
        let b = Animator::new(second).to_value(&mut cx);
        assert_ne!(a, b, "two nameless clips hash apart");
        assert_eq!(
            a,
            Animator::new(first).to_value(&mut cx),
            "and the same each time"
        );
        assert_eq!(json::to_string_compact(&a), r#"{"clip":[0,1]}"#);
    }
}
