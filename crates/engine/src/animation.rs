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

//! Sprite animation: [`Clip`]s of frames, kept in [`Clips`], and the
//! [`Animator`] that plays one on its entity's [`Sprite`](crate::Sprite)
//! (ADR-0024). A cooked atlas's clips, from Aseprite's tags, are added with
//! [`Clips::add_atlas`], each frame placed as its trimmed pixels (ADR-0035).

use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;

use nadir_assets::Handle;
use nadir_core::{GenArena, Rect, Vec2};
use nadir_ecs::{Component, Entity};
use nadir_render::{Atlas, AtlasSprite, Texture};

/// One frame of a [`Clip`]: a region of a texture, shown for `duration`,
/// and maybe the size and pivot of its quad.
///
/// Made with [`ClipFrame::new`] or [`ClipFrame::from_atlas`] outside this
/// crate, so that fields can be added later.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct ClipFrame {
    /// The texture, usually an atlas page.
    pub texture: Handle<Texture>,
    /// The region of the texture, normalized, as [`Sprite::uv`](crate::Sprite::uv).
    pub uv: Rect,
    /// How long the frame shows: more than zero.
    pub duration: Duration,
    /// The quad the frame is drawn as, which `nadir::animate` writes into
    /// the Sprite with the texture and uv: a trimmed frame's own size and
    /// pivot. `None` leaves the Sprite's size and pivot as they are, which
    /// may be those that a frame of another clip wrote.
    pub placement: Option<Placement>,
}

/// The size and pivot of a frame's quad, as [`Sprite::size`](crate::Sprite::size)
/// and [`Sprite::pivot`](crate::Sprite::pivot) hold them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    /// The size in world units.
    pub size: Vec2,
    /// The point of the quad at the entity's position, normalized.
    pub pivot: Vec2,
}

impl ClipFrame {
    /// The region `uv` of `texture`, shown for `duration`, in the quad the
    /// Sprite already has.
    pub const fn new(texture: Handle<Texture>, uv: Rect, duration: Duration) -> Self {
        Self {
            texture,
            uv,
            duration,
            placement: None,
        }
    }

    /// Frame `frame` of `sprite`, from a cooked [`Atlas`], in `texture`,
    /// which must be the frame's page, shown for `duration`: placed as
    /// [`Sprite::from_atlas`](crate::Sprite::from_atlas) places it, its
    /// trimmed size at one world unit a pixel, with the sprite's pivot where
    /// the untrimmed cell has it. So the frames of a clip, trimmed each to
    /// its own pixels, show where the sheet drew them, and mirror about the
    /// pivot. `None` if `sprite` has no such frame, or is a tile set.
    pub fn from_atlas(
        texture: Handle<Texture>,
        sprite: &AtlasSprite,
        frame: usize,
        duration: Duration,
    ) -> Option<Self> {
        let pivot = sprite.pivot()?;
        let frame = sprite.frame(frame)?;
        Some(Self {
            texture,
            uv: frame.uv(),
            duration,
            placement: Some(Placement {
                size: frame.size().as_vec2(),
                pivot: frame.pivot_in(pivot),
            }),
        })
    }
}

/// Why a clip could not be made or added.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ClipError {
    /// The clip has no frame.
    NoFrames,
    /// A frame lasts zero.
    FrameLastsZero {
        /// The frame, from 0.
        index: usize,
    },
    /// The frames together last longer than a `Duration` holds.
    TooLong,
    /// [`Clips`] has a clip of this name already.
    NameTaken(String),
    /// The textures given for an atlas's pages are not one for each.
    WrongPages {
        /// The atlas's pages.
        needed: usize,
        /// The textures given.
        given: usize,
    },
}

impl fmt::Display for ClipError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoFrames => write!(f, "a clip needs a frame"),
            Self::FrameLastsZero { index } => write!(f, "frame {index} of a clip lasts zero"),
            Self::TooLong => write!(f, "a clip's frames last longer than a Duration holds"),
            Self::NameTaken(name) => write!(f, "there is a clip named `{name}` already"),
            Self::WrongPages { needed, given } => write!(
                f,
                "the atlas has {needed} pages, and {given} textures were given for them"
            ),
        }
    }
}

impl std::error::Error for ClipError {}

/// What a [`Clip`] does at its end.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Playback {
    /// It starts again from its first frame, for as long as it plays.
    #[default]
    Loop,
    /// It stops on its last frame, and its animator sends
    /// [`AnimationFinished`].
    Once,
}

/// An animation: frames shown one after another, looping or once.
#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    frames: Vec<ClipFrame>,
    /// Where each frame ends, from the clip's start.
    ends: Vec<Duration>,
    playback: Playback,
}

impl Clip {
    /// A clip of `frames`, in order.
    ///
    /// # Panics
    ///
    /// If there are no frames, if a frame lasts zero, or if the frames
    /// together last longer than a `Duration` can hold: see
    /// [`Clip::try_new`].
    pub fn new(frames: Vec<ClipFrame>, playback: Playback) -> Self {
        match Self::try_new(frames, playback) {
            Ok(clip) => clip,
            Err(error) => panic!("{error}"),
        }
    }

    /// A clip of `frames`, in order, as [`Clip::new`] makes it.
    ///
    /// # Errors
    ///
    /// [`ClipError::NoFrames`], [`ClipError::FrameLastsZero`] or
    /// [`ClipError::TooLong`].
    pub fn try_new(frames: Vec<ClipFrame>, playback: Playback) -> Result<Self, ClipError> {
        if frames.is_empty() {
            return Err(ClipError::NoFrames);
        }
        let mut ends = Vec::with_capacity(frames.len());
        let mut end = Duration::ZERO;
        for (index, frame) in frames.iter().enumerate() {
            if frame.duration.is_zero() {
                return Err(ClipError::FrameLastsZero { index });
            }
            end = end.checked_add(frame.duration).ok_or(ClipError::TooLong)?;
            ends.push(end);
        }
        Ok(Self {
            frames,
            ends,
            playback,
        })
    }

    /// The frames, in order.
    pub fn frames(&self) -> &[ClipFrame] {
        &self.frames
    }

    /// What the clip does at its end.
    pub fn playback(&self) -> Playback {
        self.playback
    }

    /// How long the clip lasts, once through: its frames' durations added.
    pub fn duration(&self) -> Duration {
        *self.ends.last().expect("a clip has a frame")
    }

    /// The index of the frame shown `elapsed` after the clip started. A
    /// frame shows from where the one before it ends, up to but not at its
    /// own end. A looping clip starts again at each multiple of its
    /// duration; a clip that plays once shows its last frame from its end
    /// on.
    pub fn frame_at(&self, elapsed: Duration) -> usize {
        let at = match self.playback {
            Playback::Loop => from_nanos(elapsed.as_nanos() % self.duration().as_nanos()),
            Playback::Once if elapsed >= self.duration() => return self.frames.len() - 1,
            Playback::Once => elapsed,
        };
        self.ends.partition_point(|&end| end <= at)
    }
}

/// `nanos` nanoseconds, which are fewer than a `Duration` holds.
fn from_nanos(nanos: u128) -> Duration {
    let seconds = u64::try_from(nanos / 1_000_000_000).expect("less than a Duration holds");
    Duration::new(seconds, (nanos % 1_000_000_000) as u32)
}

/// The clips, by handle, and by name for those that have one: a resource
/// that the engine adds. An [`Animator`] names its clip by the handle that
/// [`Clips::add`], [`Clips::add_named`] or [`Clips::add_atlas`] gives.
///
/// It is the only place `Handle<Clip>`s come from: a clip is not an asset
/// of the `AssetServer` (ADR-0024).
pub struct Clips {
    clips: GenArena<Named>,
    names: BTreeMap<String, Handle<Clip>>,
}

/// A clip, and its name if it has one.
struct Named {
    clip: Clip,
    name: Option<String>,
}

impl Clips {
    /// No clips.
    pub fn new() -> Self {
        Self {
            clips: GenArena::new(),
            names: BTreeMap::new(),
        }
    }

    /// Adds `clip`, with no name, and returns its handle.
    pub fn add(&mut self, clip: Clip) -> Handle<Clip> {
        Handle::new(self.clips.insert(Named { clip, name: None }))
    }

    /// Adds `clip` named `name`, and returns its handle.
    ///
    /// # Errors
    ///
    /// [`ClipError::NameTaken`] if a clip has that name: none is added.
    pub fn add_named(
        &mut self,
        name: impl Into<String>,
        clip: Clip,
    ) -> Result<Handle<Clip>, ClipError> {
        let name = name.into();
        if self.names.contains_key(&name) {
            return Err(ClipError::NameTaken(name));
        }
        let handle = Handle::new(self.clips.insert(Named {
            clip,
            name: Some(name.clone()),
        }));
        self.names.insert(name, handle);
        Ok(handle)
    }

    /// Adds every clip of every sprite of `atlas`, each named by its sprite
    /// and its own name, as `sprites/person/walk`, with its frames placed
    /// by [`ClipFrame::from_atlas`] in `pages`, the textures of the atlas's
    /// pages ([`Atlas::load_pages`]). Returns their handles sprite by sprite,
    /// in the atlas's order, and each sprite's clips by name. A looping clip
    /// is [`Playback::Loop`]; the others [`Playback::Once`].
    ///
    /// # Errors
    ///
    /// [`ClipError::WrongPages`] if `pages` is not one texture a page, and
    /// [`ClipError::NameTaken`] if a clip has one of their names, as when an
    /// atlas is added twice. Then none is added.
    pub fn add_atlas(
        &mut self,
        atlas: &Atlas,
        pages: &[Handle<Texture>],
    ) -> Result<Vec<Handle<Clip>>, ClipError> {
        if pages.len() != atlas.pages().len() {
            return Err(ClipError::WrongPages {
                needed: atlas.pages().len(),
                given: pages.len(),
            });
        }
        let mut made = Vec::new();
        for sprite in atlas.sprites() {
            for clip in sprite.clips() {
                let name = format!("{}/{}", sprite.name(), clip.name());
                if self.names.contains_key(&name) {
                    return Err(ClipError::NameTaken(name));
                }
                // The atlas reader checked that each step shows a frame of
                // the sprite, which is on one of the pages.
                let frames = clip
                    .frames()
                    .iter()
                    .filter_map(|step| {
                        let page = sprite.frame(step.frame())?.page();
                        ClipFrame::from_atlas(
                            *pages.get(page)?,
                            sprite,
                            step.frame(),
                            step.duration(),
                        )
                    })
                    .collect();
                let playback = if clip.is_looping() {
                    Playback::Loop
                } else {
                    Playback::Once
                };
                made.push((name, Clip::try_new(frames, playback)?));
            }
        }
        Ok(made
            .into_iter()
            .map(|(name, clip)| {
                self.add_named(name, clip)
                    .expect("each name was checked as free, and they differ")
            })
            .collect())
    }

    /// The clip of `handle`, or `None` if it was removed.
    pub fn get(&self, handle: Handle<Clip>) -> Option<&Clip> {
        self.clips.get(handle.id()).map(|named| &named.clip)
    }

    /// The clip named `name`, such as `sprites/person/walk`.
    pub fn find(&self, name: &str) -> Option<Handle<Clip>> {
        self.names.get(name).copied()
    }

    /// The name of the clip of `handle`, if it has one and was not removed.
    pub fn name(&self, handle: Handle<Clip>) -> Option<&str> {
        self.clips.get(handle.id())?.name.as_deref()
    }

    /// Removes the clip of `handle`, and its name, and returns it.
    /// Animators that still play it stop where they are; the first such
    /// animator in the process logs a warning.
    pub fn remove(&mut self, handle: Handle<Clip>) -> Option<Clip> {
        let named = self.clips.remove(handle.id())?;
        if let Some(name) = &named.name {
            self.names.remove(name);
        }
        Some(named.clip)
    }

    /// How many clips there are.
    pub fn len(&self) -> usize {
        self.clips.len()
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.clips.is_empty()
    }
}

impl Default for Clips {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Clips {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Clips").field("len", &self.len()).finish()
    }
}

/// Plays a [`Clip`] on its entity's [`Sprite`](crate::Sprite).
///
/// The system `nadir::animate`, first in the frame stage `Animate`,
/// advances it by how long the simulation ran since the last frame (the
/// change in `RenderFrame::time`), so clips slow and stop with the clock,
/// as animated tiles do. It then writes the texture and the uv of the frame
/// it shows into the Sprite, and its size and pivot if the frame has a
/// [`Placement`], which `nadir::extract_sprites` draws.
///
/// Its time is frame state. It moves once a frame, by however the ticks
/// fell into frames, so a fixed system may call [`Animator::play`] and
/// [`Animator::restart`], but must not decide anything by
/// [`Animator::elapsed`], [`Animator::is_finished`] or what it writes into
/// the Sprite: to wait for a clip, count ticks (roadmap M4.7).
///
/// An animator that starts (new, played or restarted) is advanced on its
/// first frame by that frame's whole step, so it is first drawn up to one
/// frame into its clip: after a hitch (frames are at most 250 ms), early
/// frames can be skipped, and a clip that plays once and is no longer than
/// the step ends on its first frame.
///
/// It is not reflected until prefabs and levels (roadmap M5.11, ADR-0024).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Animator {
    clip: Handle<Clip>,
    /// How far into the clip: a looping clip's wraps, and the clip of one
    /// that plays once stops at its end.
    elapsed: Duration,
    /// A clip that plays once reached its end, and its event was sent.
    finished: bool,
}

impl Animator {
    /// Plays `clip` from its start.
    pub const fn new(clip: Handle<Clip>) -> Self {
        Self::starting_at(clip, Duration::ZERO)
    }

    /// Plays `clip` as if it had started `elapsed` ago: a looping clip from
    /// `elapsed` past its last start, and a clip that plays once at most
    /// from its end, which it then reaches on its first frame.
    ///
    /// Started at `RenderFrame::time`, from a system in a tick or in a frame,
    /// or before the first frame, a looping clip keeps in step with the
    /// animated tiles of the same frame lengths, to within the millisecond
    /// that the tiles' time is rounded to.
    pub const fn starting_at(clip: Handle<Clip>, elapsed: Duration) -> Self {
        Self {
            clip,
            elapsed,
            finished: false,
        }
    }

    /// The clip it plays.
    pub fn clip(&self) -> Handle<Clip> {
        self.clip
    }

    /// How far into its clip it is: as of the last frame, or as set since
    /// by [`Animator::starting_at`], [`Animator::play`] or
    /// [`Animator::restart`], which a looping clip wraps at its next frame.
    /// Frame state (see [`Animator`]).
    pub fn elapsed(&self) -> Duration {
        self.elapsed
    }

    /// Whether its clip plays once and reached its end by the last frame:
    /// frame state (see [`Animator`]).
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Plays `clip` from its start, unless it is the clip playing already,
    /// which goes on: a system may call this every tick. A clip that played
    /// once and finished stays finished: [`Animator::restart`] plays it
    /// again.
    pub fn play(&mut self, clip: Handle<Clip>) {
        if clip != self.clip {
            *self = Self::new(clip);
        }
    }

    /// Plays its clip from the start again.
    pub fn restart(&mut self) {
        *self = Self::new(self.clip);
    }

    /// Advances it by `by` along `clip`, its clip. Returns the index of the
    /// frame it then shows, and whether a clip that plays once reached its
    /// end in this step.
    pub(crate) fn advance(&mut self, clip: &Clip, by: Duration) -> (usize, bool) {
        let duration = clip.duration();
        let ended = match clip.playback() {
            Playback::Loop => {
                // Each is less than a Duration holds, so the sum fits.
                let nanos = self.elapsed.as_nanos() + by.as_nanos();
                self.elapsed = from_nanos(nanos % duration.as_nanos());
                false
            }
            Playback::Once => {
                self.elapsed = self.elapsed.saturating_add(by).min(duration);
                let ended = !self.finished && self.elapsed == duration;
                self.finished |= ended;
                ended
            }
        };
        (clip.frame_at(self.elapsed), ended)
    }
}

/// A clip that plays once reached its end in this frame: `nadir::animate`
/// sends it, first in the frame stage `Animate`.
///
/// Frame systems after it read it with `Events::frame_events`. Fixed
/// systems never see it: the next tick clears its tick buffer before they
/// run. Its readers run in frames, so whatever they spawn, despawn, add or
/// remove lands at a tick that depends on frame timing: change gameplay
/// state (what replays and the world hash reproduce, roadmap M4.7) by tick
/// counts in fixed systems instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnimationFinished {
    /// The entity whose animator finished.
    pub entity: Entity,
    /// The clip that finished.
    pub clip: Handle<Clip>,
}

/// The `RenderFrame::time` that the last frame animated to: the engine's
/// own resource, which `nadir::animate` keeps.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct AnimationClock(pub(crate) Duration);

impl Component for Animator {}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use nadir_core::{GenIndex, Vec2};

    use super::*;

    const MS: Duration = Duration::from_millis(1);
    const NS: Duration = Duration::from_nanos(1);

    fn texture(index: u32) -> Handle<Texture> {
        Handle::new(GenIndex::new(index, NonZeroU32::MIN))
    }

    /// Frames of `ms` milliseconds each, the i-th at uv (i, 0) to (i + 1, 1).
    fn clip(ms: &[u64], playback: Playback) -> Clip {
        let frames = (0..)
            .zip(ms)
            .map(|(i, &ms)| {
                let uv = Rect::new(Vec2::new(i as f32, 0.0), Vec2::new(i as f32 + 1.0, 1.0));
                ClipFrame::new(texture(0), uv, Duration::from_millis(ms))
            })
            .collect();
        Clip::new(frames, playback)
    }

    /// A handle for an animator under test, whose clip is passed by hand.
    fn handle() -> Handle<Clip> {
        Handle::new(GenIndex::new(0, NonZeroU32::MIN))
    }

    // -- frame_at ------------------------------------------------------------

    #[test]
    fn a_looping_clip_shows_each_frame_from_its_start_up_to_its_end() {
        // Frames of 30, 50 and 20 ms: ends at 30, 80 and 100 ms.
        let clip = clip(&[30, 50, 20], Playback::Loop);
        assert_eq!(clip.duration(), 100 * MS);
        let cases = [
            (Duration::ZERO, 0),
            (30 * MS - NS, 0),
            (30 * MS, 1),
            (80 * MS - NS, 1),
            (80 * MS, 2),
            (100 * MS - NS, 2),
            (100 * MS, 0),
            (130 * MS, 1),
            (1_000_080 * MS, 2),
            // 99,999,999 ns past the last multiple of 100 ms.
            (Duration::MAX, 2),
        ];
        for (elapsed, frame) in cases {
            assert_eq!(clip.frame_at(elapsed), frame, "at {elapsed:?}");
        }
    }

    #[test]
    fn a_clip_that_plays_once_holds_its_last_frame_from_its_end() {
        let clip = clip(&[30, 50, 20], Playback::Once);
        let cases = [
            (Duration::ZERO, 0),
            (30 * MS, 1),
            (100 * MS - NS, 2),
            (100 * MS, 2),
            (100 * MS + NS, 2),
            (130 * MS, 2),
            (Duration::MAX, 2),
        ];
        for (elapsed, frame) in cases {
            assert_eq!(clip.frame_at(elapsed), frame, "at {elapsed:?}");
        }
    }

    #[test]
    fn a_clip_needs_frames_that_last() {
        let empty = std::panic::catch_unwind(|| Clip::new(Vec::new(), Playback::Loop));
        assert!(empty.is_err(), "no frames");
        let zero = std::panic::catch_unwind(|| clip(&[30, 0], Playback::Loop));
        assert!(zero.is_err(), "a frame of zero");
        let frame =
            |duration| ClipFrame::new(texture(0), Rect::new(Vec2::ZERO, Vec2::ONE), duration);
        let long = std::panic::catch_unwind(|| {
            Clip::new(vec![frame(Duration::MAX), frame(NS)], Playback::Loop)
        });
        assert!(long.is_err(), "longer than a Duration");
        // The same, as errors.
        assert_eq!(
            Clip::try_new(Vec::new(), Playback::Loop),
            Err(ClipError::NoFrames)
        );
        assert_eq!(
            Clip::try_new(vec![frame(MS), frame(Duration::ZERO)], Playback::Once),
            Err(ClipError::FrameLastsZero { index: 1 })
        );
        assert_eq!(
            Clip::try_new(vec![frame(Duration::MAX), frame(NS)], Playback::Loop),
            Err(ClipError::TooLong)
        );
        assert_eq!(
            ClipError::FrameLastsZero { index: 1 }.to_string(),
            "frame 1 of a clip lasts zero"
        );
        // As long as a Duration holds is long enough.
        let longest = Clip::new(vec![frame(Duration::MAX - NS), frame(NS)], Playback::Loop);
        assert_eq!(longest.duration(), Duration::MAX);
        assert_eq!(longest.frame_at(Duration::MAX - NS), 1);
    }

    // -- advance -------------------------------------------------------------

    #[test]
    fn a_looping_clip_wraps_whole_cycles_and_never_ends() {
        let clip = clip(&[30, 50, 20], Playback::Loop);
        let mut animator = Animator::new(handle());
        assert_eq!(animator.advance(&clip, Duration::ZERO), (0, false));
        assert_eq!(animator.elapsed(), Duration::ZERO);
        assert_eq!(animator.advance(&clip, 30 * MS), (1, false));
        assert_eq!(animator.elapsed(), 30 * MS);
        // 280 ms: twice round, and on the start of the last frame.
        assert_eq!(animator.advance(&clip, 250 * MS), (2, false));
        assert_eq!(animator.elapsed(), 80 * MS);
        // Whole cycles change nothing.
        assert_eq!(animator.advance(&clip, 200 * MS), (2, false));
        assert_eq!(animator.elapsed(), 80 * MS);
        // Onto the end: the start again.
        assert_eq!(animator.advance(&clip, 20 * MS), (0, false));
        assert_eq!(animator.elapsed(), Duration::ZERO);
        // The longest step there is.
        assert_eq!(animator.advance(&clip, Duration::MAX), (2, false));
        assert_eq!(animator.elapsed(), 100 * MS - NS);
        assert!(!animator.is_finished());
    }

    #[test]
    fn a_clip_that_plays_once_ends_once_on_the_step_that_reaches_its_end() {
        let clip = clip(&[30, 50, 20], Playback::Once);
        let mut animator = Animator::new(handle());
        assert_eq!(animator.advance(&clip, 100 * MS - NS), (2, false));
        assert!(!animator.is_finished());
        // Exactly onto the end.
        assert_eq!(animator.advance(&clip, NS), (2, true));
        assert_eq!(animator.elapsed(), 100 * MS);
        assert!(animator.is_finished());
        // And not again.
        assert_eq!(animator.advance(&clip, Duration::ZERO), (2, false));
        assert_eq!(animator.advance(&clip, 250 * MS), (2, false));
        assert_eq!(animator.elapsed(), 100 * MS);
        assert_eq!(animator.advance(&clip, Duration::MAX), (2, false));
    }

    #[test]
    fn a_step_past_the_end_of_a_clip_that_plays_once_ends_it_there() {
        // A hitch of 250 ms over a new animator: it ends on its first step,
        // and only its last frame is drawn.
        let clip = clip(&[80, 80, 80], Playback::Once);
        let mut animator = Animator::new(handle());
        assert_eq!(animator.advance(&clip, 250 * MS), (2, true));
        assert_eq!(animator.elapsed(), 240 * MS);
        // A single frame.
        let flash = clip_of_one(16);
        let mut animator = Animator::new(handle());
        assert_eq!(animator.advance(&flash, 16 * MS), (0, true));
    }

    fn clip_of_one(ms: u64) -> Clip {
        clip(&[ms], Playback::Once)
    }

    #[test]
    fn restart_plays_a_finished_clip_again_and_play_switches_only_to_another() {
        let once = clip(&[30, 50, 20], Playback::Once);
        let mut animator = Animator::new(handle());
        assert_eq!(animator.advance(&once, 100 * MS), (2, true));
        // The same clip again: still finished.
        animator.play(handle());
        assert!(animator.is_finished());
        assert_eq!(animator.advance(&once, 16 * MS), (2, false));
        // Restarted: from its start, and it ends again.
        animator.restart();
        assert_eq!(
            (animator.elapsed(), animator.is_finished()),
            (Duration::ZERO, false)
        );
        assert_eq!(animator.advance(&once, 50 * MS), (1, false));
        assert_eq!(animator.advance(&once, 50 * MS), (2, true));
        // Another clip: from its start.
        let other = Handle::new(GenIndex::new(1, NonZeroU32::MIN));
        animator.play(other);
        assert_eq!(animator.clip(), other);
        assert_eq!(
            (animator.elapsed(), animator.is_finished()),
            (Duration::ZERO, false)
        );

        // Mid-clip, playing the same clip goes on from where it is.
        let looping = clip(&[30, 50, 20], Playback::Loop);
        let mut animator = Animator::new(handle());
        animator.advance(&looping, 50 * MS);
        animator.play(handle());
        assert_eq!(animator.elapsed(), 50 * MS);
        assert_eq!(animator.advance(&looping, 30 * MS), (2, false));
    }

    #[test]
    fn an_animator_can_start_partway_through_its_clip() {
        let looping = clip(&[100, 100, 100], Playback::Loop);
        let mut animator = Animator::starting_at(handle(), 250 * MS);
        assert_eq!(animator.advance(&looping, Duration::ZERO), (2, false));
        // Past the clip's end: wrapped.
        let mut animator = Animator::starting_at(handle(), 37_310 * MS);
        assert_eq!(animator.advance(&looping, Duration::ZERO), (1, false));
        assert_eq!(animator.elapsed(), 110 * MS);
        // A clip that plays once, started past its end, ends on its first
        // step.
        let once = clip(&[30, 50, 20], Playback::Once);
        let mut animator = Animator::starting_at(handle(), 130 * MS);
        assert_eq!(animator.advance(&once, Duration::ZERO), (2, true));
        assert_eq!(animator.elapsed(), 100 * MS);
    }

    // -- Clips ---------------------------------------------------------------

    #[test]
    fn clips_are_found_by_handle_until_removed() {
        let mut clips = Clips::new();
        assert!(clips.is_empty());
        let walk = clips.add(clip(&[100, 100], Playback::Loop));
        let jump = clips.add(clip(&[50], Playback::Once));
        assert_eq!(clips.len(), 2);
        assert_eq!(clips.get(walk).map(Clip::playback), Some(Playback::Loop));
        assert_eq!(clips.get(jump).map(Clip::duration), Some(50 * MS));
        assert_eq!(clips.remove(walk).map(|clip| clip.frames().len()), Some(2));
        assert_eq!(clips.get(walk), None);
        assert_eq!(clips.remove(walk), None);
        // A new clip in the old one's slot is not found by the old handle.
        let fall = clips.add(clip(&[10], Playback::Once));
        assert_ne!(fall, walk);
        assert_eq!(clips.get(walk), None);
        assert_eq!(format!("{clips:?}"), "Clips { len: 2 }");
    }

    #[test]
    fn a_named_clip_is_found_by_its_name_until_removed() {
        let mut clips = Clips::new();
        let walk = clips
            .add_named("sprites/person/walk", clip(&[100], Playback::Loop))
            .expect("a free name");
        let unnamed = clips.add(clip(&[10], Playback::Once));
        assert_eq!(clips.find("sprites/person/walk"), Some(walk));
        assert_eq!(clips.name(walk), Some("sprites/person/walk"));
        assert_eq!(clips.name(unnamed), None);
        assert_eq!(clips.find("sprites/person"), None);
        assert_eq!(
            clips.add_named("sprites/person/walk", clip(&[5], Playback::Loop)),
            Err(ClipError::NameTaken("sprites/person/walk".to_string()))
        );
        assert_eq!(clips.len(), 2);
        clips.remove(walk);
        assert_eq!(clips.find("sprites/person/walk"), None);
        assert_eq!(clips.name(walk), None);
        // The name is free again.
        let again = clips
            .add_named("sprites/person/walk", clip(&[5], Playback::Loop))
            .expect("a free name");
        assert_ne!(again, walk);
    }

    /// An atlas of two pages: a person whose two frames are trimmed apart,
    /// with two clips, and a tree with none.
    fn atlas() -> Atlas {
        use nadir_assets::{Asset, DecodeCtx};
        use nadir_codec::{Limits, json, kbj};
        let text = r#"{ "version": 2,
            "pages": [ { "path": "atlas/sprites_0.qoi", "w": 64, "h": 64 },
                       { "path": "atlas/sprites_1.qoi", "w": 64, "h": 64 } ],
            "sprites": [
              { "name": "sprites/person",
                "frames": [ { "page": 0, "x": 1, "y": 1, "w": 10, "h": 20, "ox": 1, "oy": 0, "sw": 12, "sh": 20 },
                            { "page": 1, "x": 1, "y": 1, "w": 9, "h": 18, "ox": 2, "oy": 2, "sw": 12, "sh": 20 } ],
                "pivot": [6.0, 20.0],
                "clips": [ { "name": "stand", "loop": true, "frames": [[0, 100]] },
                           { "name": "wave", "loop": false, "frames": [[1, 40], [0, 60]] } ] },
              { "name": "sprites/tree",
                "frames": [ { "page": 0, "x": 20, "y": 1, "w": 24, "h": 44, "ox": 0, "oy": 4, "sw": 24, "sh": 48 } ],
                "pivot": [12.0, 48.0] } ] }"#;
        let value = json::parse(text, Limits::default()).expect("the test's JSON");
        let bytes = kbj::encode(&value, Limits::default()).expect("encodable");
        Atlas::decode(
            &bytes,
            &DecodeCtx::new("atlas/sprites.kbj", Limits::default()),
        )
        .expect("an atlas")
    }

    #[test]
    fn an_atlas_s_clips_are_added_by_name_each_frame_placed_on_its_page() {
        let atlas = atlas();
        let pages = [texture(7), texture(8)];
        let mut clips = Clips::new();
        let added = clips.add_atlas(&atlas, &pages).expect("the clips");
        let stand = clips.find("sprites/person/stand").expect("stand");
        let wave = clips.find("sprites/person/wave").expect("wave");
        assert_eq!(added, [stand, wave]);
        assert_eq!(clips.len(), 2);

        let stand = clips.get(stand).expect("a clip");
        assert_eq!(stand.playback(), Playback::Loop);
        assert_eq!(stand.duration(), 100 * MS);
        let wave = clips.get(wave).expect("a clip");
        assert_eq!(wave.playback(), Playback::Once);
        let person = atlas.sprite("sprites/person").expect("the person");
        let shown: Vec<(Handle<Texture>, Rect, Duration)> = wave
            .frames()
            .iter()
            .map(|frame| (frame.texture, frame.uv, frame.duration))
            .collect();
        assert_eq!(
            shown,
            [
                (pages[1], person.frames()[1].uv(), 40 * MS),
                (pages[0], person.frames()[0].uv(), 60 * MS),
            ]
        );
        // Each frame its own quad, with the pivot where the cell has it: a
        // frame 2 pixels in from the left, 9 wide, has the pivot 6 - 2 = 4
        // pixels in.
        assert_eq!(
            wave.frames()[0].placement,
            Some(Placement {
                size: Vec2::new(9.0, 18.0),
                pivot: Vec2::new(4.0 / 9.0, 18.0 / 18.0),
            })
        );
        assert_eq!(
            wave.frames()[1].placement,
            Some(Placement {
                size: Vec2::new(10.0, 20.0),
                pivot: Vec2::new(0.5, 1.0),
            })
        );
    }

    #[test]
    fn an_atlas_s_clips_are_added_all_or_none() {
        let atlas = atlas();
        let mut clips = Clips::new();
        assert_eq!(
            clips.add_atlas(&atlas, &[texture(7)]),
            Err(ClipError::WrongPages {
                needed: 2,
                given: 1
            })
        );
        assert!(clips.is_empty());
        // One name already taken: none is added.
        clips
            .add_named("sprites/person/wave", clip(&[5], Playback::Loop))
            .expect("a free name");
        assert_eq!(
            clips.add_atlas(&atlas, &[texture(7), texture(8)]),
            Err(ClipError::NameTaken("sprites/person/wave".to_string()))
        );
        assert_eq!(clips.len(), 1);
        assert_eq!(clips.find("sprites/person/stand"), None);
    }

    #[test]
    fn a_frame_of_an_atlas_is_placed_as_a_sprite_of_it_would_be() {
        let atlas = atlas();
        let person = atlas.sprite("sprites/person").expect("the person");
        let frame = ClipFrame::from_atlas(texture(8), person, 1, 40 * MS).expect("a frame");
        let sprite = crate::Sprite::from_atlas(texture(8), person, 1).expect("a sprite");
        let placement = frame.placement.expect("placed");
        assert_eq!(
            (frame.uv, placement.size, placement.pivot),
            (sprite.uv, sprite.size, sprite.pivot)
        );
        assert!(ClipFrame::from_atlas(texture(8), person, 2, MS).is_none());
    }
}
