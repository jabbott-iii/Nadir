//! The engine's components: where entities are, how they move, what they
//! look like and what they collide with (ADR-0014, ADR-0022).

use nadir_assets::Handle;
use nadir_collision::Shape;
use nadir_core::{Color, Rect, Vec2};
use nadir_ecs::{Component, Entity};
use nadir_render::{Flip, SortMode, SpriteDraw, Texture};

nadir_scene::reflect_struct! {
    /// Where an entity is, in world pixels (+Y down), as the last tick left
    /// it. For a sprite, it is where its pivot is: its feet.
    #[derive(Clone, Copy, Debug, Default, PartialEq)]
    pub struct Transform {
        /// The entity's position.
        pub position: Vec2,
    }
}

impl Transform {
    /// An entity at `position`.
    pub const fn at(position: Vec2) -> Self {
        Self { position }
    }
}

/// Where an entity was at the start of the last tick: a frame between two
/// ticks draws it part of the way from there to its [`Transform`], by the
/// frame's `alpha`.
///
/// The engine keeps it. The system `nadir::snapshot` copies each
/// `Transform` into it at the start of every tick. It also gives one to
/// every entity with a [`Sprite`] that has none, and to the entity each
/// drawn [`Parent`] chain ends at, so a game does not add it. To move an
/// entity at once, with nothing drawn on the way (a teleport), write both
/// its `Transform` and this, in a fixed system: a drawn entity has one
/// from the start of its first tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PrevTransform(pub Transform);

nadir_scene::reflect_newtype! {
    /// How fast an entity moves, in pixels a second. The system
    /// `nadir::movement` moves it by `velocity × dt` each tick: with a
    /// [`Collider`], it slides along what it meets.
    #[derive(Clone, Copy, Debug, Default, PartialEq)]
    pub struct Velocity(pub Vec2);
}

/// What an entity looks like: a sprite drawn at its [`Transform`], between
/// ticks part of the way from its [`PrevTransform`].
///
/// It holds what [`SpriteDraw`] holds but the position. It is not reflected
/// until prefabs bring the `Handle` encoding (roadmap M5.6, ADR-0020).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sprite {
    /// The texture, usually an atlas page.
    pub texture: Handle<Texture>,
    /// The region of the texture, normalized: (0, 0) to (1, 1) is all of
    /// it.
    pub uv: Rect,
    /// The size in world units.
    pub size: Vec2,
    /// The point of the sprite at the entity's position, normalized:
    /// (0.5, 1) is the middle of its bottom edge, its feet.
    pub pivot: Vec2,
    /// Radians, clockwise on screen, about the pivot.
    pub rotation: f32,
    /// A tint, sRGB, multiplied with the texture in linear light.
    pub color: Color,
    /// Higher layers are drawn in front of lower ones.
    pub layer: u8,
    /// How it is ordered within its layer.
    pub sort: SortMode,
    /// Which way its texture is mirrored.
    pub flip: Flip,
}

impl Sprite {
    /// A sprite of all of `texture`, `size` large, with its feet at the
    /// entity's position: untinted, unrotated, y-sorted in layer 0, as
    /// [`SpriteDraw::new`] makes one.
    pub fn new(texture: Handle<Texture>, size: Vec2) -> Self {
        let draw = SpriteDraw::new(texture, Vec2::ZERO, size);
        Self {
            texture: draw.texture,
            uv: draw.uv,
            size: draw.size,
            pivot: draw.pivot,
            rotation: draw.rotation,
            color: draw.color,
            layer: draw.layer,
            sort: draw.sort,
            flip: draw.flip,
        }
    }

    /// The sprite to draw with its pivot at `position`.
    pub fn draw(&self, position: Vec2) -> SpriteDraw {
        SpriteDraw {
            texture: self.texture,
            uv: self.uv,
            position,
            size: self.size,
            pivot: self.pivot,
            rotation: self.rotation,
            color: self.color,
            layer: self.layer,
            sort: self.sort,
            flip: self.flip,
        }
    }
}

/// What an entity collides with: a [`nadir_collision::Collider`], offset
/// from its [`Transform`]. The engine builds the
/// [`Collisions`](crate::Collisions) world from these each tick.
///
/// A child's solid collider ([`Parent`]) is in its own parent's way when
/// the child's layer is in the parent's mask, as `Collider::solid` makes
/// it: the parent moves while the child is still where the last tick left
/// it, so a move toward the child stops at it, as at a wall. Give the child
/// a layer outside its parent's mask, or make it a trigger, which stops
/// nothing.
///
/// It is not reflected until prefabs give layers names (roadmap M5.6).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Collider(pub nadir_collision::Collider);

impl Collider {
    /// A solid `shape`, on every layer, colliding with every layer.
    pub fn solid(shape: Shape) -> Self {
        Self(nadir_collision::Collider::solid(shape))
    }

    /// A trigger `shape`, on every layer, detecting every layer.
    pub fn trigger(shape: Shape) -> Self {
        Self(nadir_collision::Collider::trigger(shape))
    }
}

impl From<nadir_collision::Collider> for Collider {
    fn from(collider: nadir_collision::Collider) -> Self {
        Self(collider)
    }
}

nadir_scene::reflect_struct! {
    /// The entity the player controls: a tag.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct Player;
}

/// Puts an entity at another's position plus `offset`: the system
/// `nadir::parent_offsets` writes its [`Transform`] after each tick's
/// movement, and frames draw it at its parent's drawn position plus the
/// offset.
///
/// A parent may have a parent of its own: a child goes with the first
/// entity up the chain that has none, its root, by the offsets along the
/// way, up to 8 steps. Entities on the way need no `Transform`. A child
/// whose chain is longer, or loops, or ends at an entity without a
/// `Transform` (one that was despawned, say) is left where it is.
///
/// It is not reflected until saves bring the `Entity` encoding (roadmap
/// M9.4, ADR-0020).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Parent {
    /// The entity it goes with.
    pub entity: Entity,
    /// Where it is from that entity, in pixels.
    pub offset: Vec2,
}

nadir_scene::reflect_newtype! {
    /// An entity's id in saves, which stays the same from one run to the
    /// next, unlike its [`Entity`]. Saves (roadmap M9.4) store only
    /// entities that have one.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
    pub struct SaveId(pub u64);
}

impl Component for Transform {}
impl Component for PrevTransform {}
impl Component for Velocity {}
impl Component for Sprite {}
impl Component for Collider {}
impl Component for Player {}
impl Component for Parent {}
impl Component for SaveId {}
