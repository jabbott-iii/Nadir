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

//! The engine's components: where entities are, how they move, what they
//! look like, what they collide with, and what they sound like (ADR-0014,
//! ADR-0022).

use nadir_assets::Handle;
use nadir_audio::{Bus, Music, PlayParams, Sound};
use nadir_collision::Shape;
use nadir_core::{Color, Rect, Vec2};
use nadir_ecs::{Component, Entity};
use nadir_render::{AtlasSprite, Flip, SortMode, SpriteDraw, Texture};

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
/// It holds what [`SpriteDraw`] holds but the position. It is reflected by
/// hand, its texture as its page's path, and prefabs may name a frame of a
/// cooked atlas instead (`crate::reflected`, ADR-0038); a hash leaves out
/// what `nadir::animate` writes, below.
///
/// On an entity with an [`Animator`](crate::Animator), the system
/// `nadir::animate` writes its `texture` and `uv` each frame, and its `size`
/// and `pivot` when the frame shown has a [`Placement`](crate::Placement), as
/// a cooked atlas's clips' frames do; on one with a [`Facing`], its
/// `flip.x`.
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
    /// Which way it is mirrored, about its pivot (render's
    /// [`SpriteDraw::flip`]).
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

    /// A sprite of frame `frame` of `sprite`, from a cooked
    /// [`Atlas`](nadir_render::Atlas), in `texture`, which must be the
    /// frame's page, with the sprite's pivot at the entity's position: the
    /// frame's trimmed size, one world unit a pixel, placed so that its
    /// pixels are where the untrimmed cell would have them, up to the
    /// rounding of its normalized pivot; otherwise as [`Sprite::new`] makes
    /// one. `None` if `sprite` has no such frame, or is a tile set.
    ///
    /// Flipped, it mirrors about its pivot, so the trimmed quad lands where
    /// the mirrored cell's pixels would. Animated, by a clip of
    /// [`Clips::add_atlas`](crate::Clips::add_atlas), each frame is placed
    /// by its own trimmed size and pivot ([`ClipFrame::from_atlas`](crate::ClipFrame::from_atlas)).
    pub fn from_atlas(
        texture: Handle<Texture>,
        sprite: &AtlasSprite,
        frame: usize,
    ) -> Option<Self> {
        let pivot = sprite.pivot()?;
        let frame = sprite.frame(frame)?;
        let mut drawn = Self::new(texture, frame.size().as_vec2());
        drawn.uv = frame.uv();
        drawn.pivot = frame.pivot_in(pivot);
        Some(drawn)
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
/// It is reflected by hand, its layers by the names
/// [`App::collision_layers`](crate::App::collision_layers) gives them
/// (`crate::reflected`, ADR-0038).
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

nadir_scene::reflect_enum! {
    /// Which way an entity faces. Art is drawn facing right: the system
    /// `nadir::animate` mirrors the [`Sprite`] of an entity that faces left
    /// (`flip.x`), and unmirrors it whichever other way it faces. Only the
    /// entity's own Sprite: a [`Parent`]'s children need their own.
    ///
    /// It is simulation state, reflected: set it in fixed systems. Set in a
    /// frame system, it mirrors the Sprite from the next frame.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub enum Facing {
        /// Toward +Y, down the screen.
        #[default]
        Down,
        /// Toward −Y, up the screen.
        Up,
        /// Toward −X: the Sprite is mirrored.
        Left,
        /// Toward +X, as the art is drawn.
        Right,
    }
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

/// Where the game hears from: the first entity with one and a
/// [`Transform`], where it is drawn, between ticks part of the way from its
/// [`PrevTransform`]. With none, the camera's position
/// ([`RenderFrame::camera`](nadir_render::RenderFrame::camera)) is.
///
/// It is not reflected: a game gives it in code, not in prefabs or levels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AudioListener;

/// A sound that plays at its entity, and follows it as it moves: the system
/// `nadir::sound` starts it once its sound has loaded, while the entity has
/// a [`Transform`] and is within
/// [`AudioConfig::max_dist`](nadir_audio::AudioConfig::max_dist) of the
/// listener ([`AudioListener`]), 800 pixels, where it can be heard. It
/// stops, fading out over 100 ms, when the entity is despawned or loses
/// the emitter or its `Transform`, or is more than 64 pixels past that
/// distance; back within it, it starts again. A sound that does not loop
/// plays once each time it starts.
///
/// Changed, it follows: a new `sound`, `bus`, `looped`, `priority` or
/// `max_instances` stops the voice and starts the new one, and a new
/// `volume` or `pitch` reaches the voice as it plays. Its `pan` is not
/// used: where it is pans it.
///
/// A voice taken for another when the voices run short stays silent until
/// the emitter starts again. [`PlayParams::priority`] protects it only
/// from plays of a lower priority: [`AudioEmitter::looped`]'s 192 from
/// sounds played with the default, 128. A sound that failed to load, or
/// that the asset server does not hold, is warned of once, with the entity
/// and its path, and not tried again until the emitter changes.
///
/// It is not reflected: a game gives it in code, not in prefabs or levels.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct AudioEmitter {
    /// The sound it plays.
    pub sound: Handle<Sound>,
    /// How it plays it.
    pub params: PlayParams,
}

impl AudioEmitter {
    /// An emitter of `sound`, played as `params` say.
    pub const fn new(sound: Handle<Sound>, params: PlayParams) -> Self {
        Self { sound, params }
    }

    /// An emitter of `sound` looped on [`Bus::Ambience`], for a sound of
    /// the place, such as water or a fire: at priority 192, and with no
    /// limit to the voices of the sound, so that the sound's limit does not
    /// make its emitters take one another's voices; when all the mixer's
    /// voices play, a play of the same priority or higher still takes the
    /// oldest of them ([`Audio::play_sound`](nadir_audio::Audio::play_sound)).
    /// Otherwise as [`PlayParams::default`] plays.
    pub fn looped(sound: Handle<Sound>) -> Self {
        Self::new(
            sound,
            PlayParams {
                bus: Bus::Ambience,
                looped: true,
                priority: 192,
                max_instances: u8::MAX,
                ..PlayParams::default()
            },
        )
    }
}

/// A box, its entity's position ± `half` in pixels, whose music plays
/// while the listener ([`AudioListener`]) is in it: the system
/// `nadir::sound` crossfades to `music` over `crossfade_ms` when the
/// listener comes into it. Where areas overlap, the one playing stays
/// while the listener is in it, and otherwise the first of them that holds
/// it plays. Outside every area, the music plays on; coming back into the
/// area of the track that plays, it goes on as it is (ADR-0046).
///
/// A track still loading plays once it has loaded. One that failed to
/// load, or that the asset server does not hold, is warned of once, with
/// the entity and its path, and not tried again until the area changes:
/// the music plays on as it is.
///
/// It is not reflected: a game gives it in code, not in prefabs or levels.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct MusicArea {
    /// The track it plays, looped, on [`Bus::Music`].
    pub music: Handle<Music>,
    /// Half its size, from its entity's position.
    pub half: Vec2,
    /// How long the crossfade to its track takes, in milliseconds.
    pub crossfade_ms: u32,
}

impl MusicArea {
    /// An area of `music`, its entity's position ± `half`, that crossfades
    /// to it over 1,500 ms.
    pub const fn new(music: Handle<Music>, half: Vec2) -> Self {
        Self {
            music,
            half,
            crossfade_ms: 1_500,
        }
    }
}

impl Component for Transform {}
impl Component for PrevTransform {}
impl Component for Velocity {}
impl Component for Sprite {}
impl Component for Collider {}
impl Component for Player {}
impl Component for Facing {}
impl Component for Parent {}
impl Component for SaveId {}
impl Component for AudioListener {}
impl Component for AudioEmitter {}
impl Component for MusicArea {}

#[cfg(test)]
mod tests {
    use nadir_assets::{Asset, DecodeCtx};
    use nadir_codec::{Limits, json, kbj};
    use nadir_render::Atlas;

    use super::*;

    #[test]
    fn a_sprite_of_a_trimmed_frame_covers_the_pixels_of_its_cell() {
        let text = r#"{ "version": 2,
            "pages": [ { "path": "atlas/props_0.qoi", "w": 64, "h": 64 } ],
            "sprites": [ { "name": "props/rock",
              "frames": [ { "page": 0, "x": 3, "y": 5, "w": 14, "h": 10, "ox": 1, "oy": 2, "sw": 16, "sh": 12 } ],
              "pivot": [8.0, 12.0] } ] }"#;
        let value = json::parse(text, Limits::default()).expect("the test's JSON");
        let bytes = kbj::encode(&value, Limits::default()).expect("encodable");
        let atlas = Atlas::decode(
            &bytes,
            &DecodeCtx::new("atlas/props.kbj", Limits::default()),
        )
        .expect("an atlas");
        let rock = atlas.sprite("props/rock").expect("the rock");
        let texture = Handle::new(nadir_core::GenIndex::new(0, std::num::NonZeroU32::MIN));
        assert!(Sprite::from_atlas(texture, rock, 1).is_none());
        let sprite = Sprite::from_atlas(texture, rock, 0).expect("a frame");
        assert_eq!(sprite.size, Vec2::new(14.0, 10.0));
        assert_eq!(sprite.uv, rock.frames()[0].uv());
        // At the rock's feet, the quad's corner is the cell's, moved by the
        // trim.
        let feet = Vec2::new(40.0, 30.0);
        let draw = sprite.draw(feet);
        let corner = draw.position - draw.pivot * draw.size;
        assert_eq!(corner, feet - Vec2::new(8.0, 12.0) + Vec2::new(1.0, 2.0));
        assert_eq!((sprite.layer, sprite.color), (0, Color::WHITE));
    }

    #[test]
    fn a_looped_emitter_loops_on_the_ambience_bus_at_192_with_no_limit() {
        let sound = Handle::new(nadir_core::GenIndex::new(0, std::num::NonZeroU32::MIN));
        let params = PlayParams {
            bus: Bus::Ambience,
            looped: true,
            priority: 192,
            max_instances: u8::MAX,
            ..PlayParams::default()
        };
        assert_eq!(
            AudioEmitter::looped(sound),
            AudioEmitter::new(sound, params)
        );
    }
}
