//! Sprite animation: [`Clip`]s of frames, kept in [`Clips`], and the
//! [`Animator`] that plays one on its entity's [`Sprite`](crate::Sprite)
//! (ADR-0024).

use std::fmt;
use std::time::Duration;

use nadir_assets::Handle;
use nadir_core::{GenArena, Rect};
use nadir_ecs::{Component, Entity};
use nadir_render::Texture;

/// One frame of a [`Clip`]: a region of a texture, shown for `duration`.
///
/// Made with [`ClipFrame::new`] outside this crate, so that fields can be
/// added later (a trimmed frame's size and pivot, roadmap M5.5).
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct ClipFrame {
    /// The texture, usually an atlas page.
    pub texture: Handle<Texture>,
    /// The region of the texture, normalized, as [`Sprite::uv`](crate::Sprite::uv).
    pub uv: Rect,
    /// How long the frame shows: more than zero.
    pub duration: Duration,
}

impl ClipFrame {
    /// The region `uv` of `texture`, shown for `duration`.
    pub const fn new(texture: Handle<Texture>, uv: Rect, duration: Duration) -> Self {
        Self {
            texture,
            uv,
            duration,
        }
    }
}

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
    /// together last longer than a `Duration` can hold.
    pub fn new(frames: Vec<ClipFrame>, playback: Playback) -> Self {
        assert!(!frames.is_empty(), "a clip needs a frame");
        let mut ends = Vec::with_capacity(frames.len());
        let mut end = Duration::ZERO;
        for (index, frame) in frames.iter().enumerate() {
            assert!(
                !frame.duration.is_zero(),
                "frame {index} of a clip lasts zero"
            );
            end = match end.checked_add(frame.duration) {
                Some(end) => end,
                None => panic!("a clip's frames last longer than a Duration holds"),
            };
            ends.push(end);
        }
        Self {
            frames,
            ends,
            playback,
        }
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

/// The clips, by handle: a resource that the engine adds. An [`Animator`]
/// names its clip by the handle that [`Clips::add`] gives.
///
/// It is the only place `Handle<Clip>`s come from: a clip is not an asset
/// of the `AssetServer` (ADR-0024).
pub struct Clips {
    clips: GenArena<Clip>,
}

impl Clips {
    /// No clips.
    pub fn new() -> Self {
        Self {
            clips: GenArena::new(),
        }
    }

    /// Adds `clip`, and returns its handle.
    pub fn add(&mut self, clip: Clip) -> Handle<Clip> {
        Handle::new(self.clips.insert(clip))
    }

    /// The clip of `handle`, or `None` if it was removed.
    pub fn get(&self, handle: Handle<Clip>) -> Option<&Clip> {
        self.clips.get(handle.id())
    }

    /// Removes the clip of `handle`, and returns it. Animators that still
    /// play it stop where they are; the first such animator in the process
    /// logs a warning.
    pub fn remove(&mut self, handle: Handle<Clip>) -> Option<Clip> {
        self.clips.remove(handle.id())
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
/// it shows into the Sprite, which `nadir::extract_sprites` draws.
///
/// Its time is frame state. It moves once a frame, by however the ticks
/// fell into frames, so a fixed system may call [`Animator::play`] and
/// [`Animator::restart`], but must not decide anything by
/// [`Animator::elapsed`], [`Animator::is_finished`] or the Sprite's
/// texture and uv: to wait for a clip, count ticks (roadmap M4.7).
///
/// An animator that starts (new, played or restarted) is advanced on its
/// first frame by that frame's whole step, so it is first drawn up to one
/// frame into its clip: after a hitch (frames are at most 250 ms), early
/// frames can be skipped, and a clip that plays once and is no longer than
/// the step ends on its first frame.
///
/// It is not reflected until prefabs and levels (roadmap M5.6, ADR-0024).
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
}
