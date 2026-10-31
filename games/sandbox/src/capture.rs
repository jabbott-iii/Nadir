//! `--capture DIR`: draws the scene offscreen, with no window, one tick a
//! frame, and writes its frames and what they showed into `DIR`, for the
//! demo page (ADR-0029). `cargo xtask site` runs it for each scene and
//! makes videos of the frames.
//!
//! - `frames.rgba`: [`FRAMES`] frames, one after another, each [`SIZE`]
//!   pixels of RGBA8, sRGB-encoded, from the top row, 4 bytes a pixel. A
//!   frame is a tick, so they play at [`TICK_HZ`] frames a second.
//! - `capture.json`: the scene, the frames' size, rate and count, the
//!   ticks, the world's hash after them, what the frames drew, how long
//!   the ticks took, and the renderer.
//!
//! In the field, the person walks [`TOUR`], unless `--replay` plays a
//! replay in place of the keys. In the other scenes the people walk by
//! themselves.

use std::error::Error;
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use nadir::log::info;
use nadir::prelude::*;
use nadir::{Event, TICK_HZ};

use crate::{Report, Scene};

/// One tick's length, as a frame: it runs one tick.
const TICK: Duration = Duration::from_nanos(16_666_667);

/// The frames, one a tick: 10 seconds.
pub(crate) const FRAMES: u32 = 600;

/// The frames' size: the virtual resolution, so each pixel of the game is
/// one of the frames'.
pub(crate) const SIZE: UVec2 = UVec2::new(480, 270);

/// The person's walk in the field, from the start, two pixels a tick:
/// each step holds its keys down for its ticks, and they fill the
/// capture. Down to the path and along it to the east, past the pond, to
/// where it turns down, in column 50; down it, past the flowers and
/// through the gap in the tall walls of row 36; then up and west, sliding
/// along the wall's face until it ends.
const TOUR: &[(u32, &[Key])] = &[
    (20, &[]),
    (28, &[Key::S, Key::D]),
    (264, &[Key::D]),
    (200, &[Key::S]),
    (88, &[Key::A, Key::W]),
];

/// Captures `app`, which shows `scene`, into the directory `dir`, which it
/// makes. `tour` walks the person [`TOUR`] in the field.
///
/// # Errors
///
/// If the frames cannot be drawn offscreen, as on a system with no
/// offscreen OpenGL context, or the files cannot be written.
pub(crate) fn capture(
    app: App,
    scene: Scene,
    dir: &Path,
    tour: bool,
) -> Result<(), Box<dyn Error>> {
    let cannot = |error| format!("cannot write the capture into {}: {error}", dir.display());
    fs::create_dir_all(dir).map_err(cannot)?;
    // The once-a-second report goes by the wall clock, which this run does
    // not keep to.
    app.world().resource_mut::<Report>().rate.reported_at = Duration::MAX;
    let mut offscreen = app.offscreen(SIZE)?;
    info!(
        "capturing {FRAMES} frames of {} × {} of the scene {} on {}",
        SIZE.x,
        SIZE.y,
        scene.name(),
        offscreen.renderer()
    );
    // A frame with no tick, which makes the textures, so that the first
    // captured frame shows them.
    offscreen.frame(Duration::ZERO, &[])?;

    let mut frames = BufWriter::new(File::create(dir.join("frames.rgba")).map_err(cannot)?);
    let mut seen = Seen::default();
    let mut held: &[Key] = &[];
    let mut events = Vec::new();
    let started = Instant::now();
    for frame in 0..FRAMES {
        events.clear();
        if tour && scene == Scene::Field {
            let keys = keys_at(frame);
            key_changes(held, keys, &mut events);
            held = keys;
        }
        let pixels = offscreen.frame(TICK, &events)?;
        frames.write_all(&pixels).map_err(cannot)?;
        let world = offscreen.app().world();
        seen.add(
            *world.resource::<FrameStats>(),
            *world.resource::<TickStats>(),
        );
    }
    frames.flush().map_err(cannot)?;
    let hash = offscreen.app().world_hash();
    let json = seen.json(scene, hash, offscreen.renderer());
    fs::write(dir.join("capture.json"), json).map_err(cannot)?;
    info!(
        "captured {FRAMES} frames, {} ticks, in {:.1} s; world hash {hash:016x}",
        seen.ticks,
        started.elapsed().as_secs_f64()
    );
    Ok(())
}

/// The keys [`TOUR`] holds down in tick `tick`, from 0: none after its
/// end.
fn keys_at(tick: u32) -> &'static [Key] {
    let mut start = 0;
    for &(ticks, keys) in TOUR {
        start += ticks;
        if tick < start {
            return keys;
        }
    }
    &[]
}

/// Adds to `out` the key events that take the keys held down from
/// `before` to `now`: releases, then presses.
fn key_changes(before: &[Key], now: &[Key], out: &mut Vec<Event>) {
    let event = |key, pressed| Event::Key {
        key,
        pressed,
        repeat: false,
    };
    let released = before.iter().filter(|key| !now.contains(key));
    out.extend(released.map(|&key| event(key, false)));
    let pressed = now.iter().filter(|key| !before.contains(key));
    out.extend(pressed.map(|&key| event(key, true)));
}

/// What the captured frames drew, and how long their ticks took.
#[derive(Default)]
struct Seen {
    frames: u64,
    ticks: u64,
    sprites: u64,
    most_sprites: u32,
    batches: u64,
    tiles: u64,
    tick_time: Duration,
    longest_tick: Duration,
}

impl Seen {
    /// Adds a frame's.
    fn add(&mut self, frame: FrameStats, ticks: TickStats) {
        self.frames += 1;
        self.ticks += u64::from(ticks.ticks);
        self.sprites += u64::from(frame.sprites);
        self.most_sprites = self.most_sprites.max(frame.sprites);
        self.batches += u64::from(frame.batches);
        self.tiles += u64::from(frame.tiles);
        self.tick_time += ticks.time;
        self.longest_tick = self.longest_tick.max(ticks.longest);
    }

    /// `capture.json`: the means are a frame's, or a tick's, and the times
    /// are in milliseconds.
    fn json(&self, scene: Scene, hash: u64, renderer: &str) -> String {
        let frames = self.frames.max(1) as f64;
        let ticks = self.ticks.max(1) as f64;
        let mut json = String::from("{\n");
        let mut field = |name: &str, value: String| {
            // Writing to a `String` cannot fail.
            let _ = writeln!(json, "  \"{name}\": {value},");
        };
        field("scene", quoted(scene.name()));
        field("width", SIZE.x.to_string());
        field("height", SIZE.y.to_string());
        field("fps", TICK_HZ.to_string());
        field("frames", self.frames.to_string());
        field("ticks", self.ticks.to_string());
        field("world_hash", quoted(&format!("{hash:016x}")));
        field(
            "mean_sprites",
            format!("{:.1}", self.sprites as f64 / frames),
        );
        field("most_sprites", self.most_sprites.to_string());
        field(
            "mean_batches",
            format!("{:.1}", self.batches as f64 / frames),
        );
        field("mean_tiles", format!("{:.1}", self.tiles as f64 / frames));
        field(
            "mean_tick_ms",
            format!("{:.3}", self.tick_time.as_secs_f64() * 1000.0 / ticks),
        );
        field(
            "longest_tick_ms",
            format!("{:.3}", self.longest_tick.as_secs_f64() * 1000.0),
        );
        let _ = writeln!(json, "  \"renderer\": {}\n}}", quoted(renderer));
        json
    }
}

/// `text` as a JSON string.
fn quoted(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c < ' ' => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tour_holds_each_steps_keys_for_its_ticks_and_then_none() {
        assert_eq!(keys_at(0), &[] as &[Key]);
        assert_eq!(keys_at(19), &[] as &[Key]);
        assert_eq!(keys_at(20), &[Key::S, Key::D]);
        assert_eq!(keys_at(47), &[Key::S, Key::D]);
        assert_eq!(keys_at(48), &[Key::D]);
        let length: u32 = TOUR.iter().map(|&(ticks, _)| ticks).sum();
        assert!(length <= FRAMES, "the tour outlasts the capture");
        assert_eq!(keys_at(length - 1), TOUR[TOUR.len() - 1].1);
        assert_eq!(keys_at(length), &[] as &[Key]);
        assert_eq!(keys_at(u32::MAX), &[] as &[Key]);
    }

    #[test]
    fn the_tour_walks_down_the_path_through_the_gap_and_slides_along_the_wall() {
        // As `capture` runs it: a frame with no tick, then a tick a frame.
        let mut app = crate::tests::scene(Scene::Field);
        let player = crate::tests::player(&app);
        app.update(Duration::ZERO, &[]);
        let mut held: &[Key] = &[];
        let mut events = Vec::new();
        let mut at = Vec::new();
        for frame in 0..FRAMES {
            events.clear();
            let keys = keys_at(frame);
            key_changes(held, keys, &mut events);
            held = keys;
            app.update(TICK, &events);
            at.push(crate::tests::position(&app, player));
        }
        let cell = |p: Vec2| ((p.x / 16.0).floor() as i32, (p.y / 16.0).floor() as i32);
        // The frame at the end of step `step`.
        let end = |step: usize| {
            TOUR[..=step]
                .iter()
                .map(|&(ticks, _)| ticks as usize)
                .sum::<usize>()
                - 1
        };
        assert_eq!(
            cell(at[end(2)]),
            (50, 12),
            "on the path where it turns down"
        );
        assert_eq!(
            cell(at[end(3)]),
            (50, 37),
            "down it, through the gap in row 36"
        );
        // Held against the wall's face while they go west.
        let slide = &at[end(3) + 10..end(3) + 50];
        assert!(
            slide
                .windows(2)
                .all(|pair| pair[1].y == pair[0].y && pair[1].x < pair[0].x),
            "{slide:?}"
        );
        assert_eq!(cell(slide[0]).1, 37);
        // Then past its end, at column 44, up and west.
        let last = at[at.len() - 1];
        assert!(last.x < 44.0 * 16.0 && last.y < slide[0].y, "{last:?}");
    }

    #[test]
    fn key_changes_release_what_was_let_go_and_press_what_is_new() {
        let mut out = Vec::new();
        key_changes(&[Key::D], &[Key::D, Key::W], &mut out);
        assert_eq!(
            out,
            [Event::Key {
                key: Key::W,
                pressed: true,
                repeat: false
            }]
        );
        out.clear();
        key_changes(&[Key::D, Key::W], &[Key::S], &mut out);
        let changes: Vec<(Key, bool)> = out
            .iter()
            .map(|event| match *event {
                Event::Key { key, pressed, .. } => (key, pressed),
                ref other => panic!("not a key event: {other:?}"),
            })
            .collect();
        assert_eq!(changes, [(Key::D, false), (Key::W, false), (Key::S, true)]);
        out.clear();
        key_changes(&[Key::S], &[Key::S], &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn the_json_names_every_number_and_quotes_the_strings() {
        let mut seen = Seen::default();
        let frame = FrameStats {
            sprites: 10,
            batches: 2,
            tiles: 600,
            ..FrameStats::default()
        };
        let ticks = TickStats {
            ticks: 1,
            time: Duration::from_micros(250),
            longest: Duration::from_micros(250),
        };
        seen.add(frame, ticks);
        seen.add(
            FrameStats {
                sprites: 20,
                ..frame
            },
            ticks,
        );
        let json = seen.json(Scene::Field, 0xabc, "llvmpipe \"x\\y\"");
        assert_eq!(
            json,
            "{\n  \"scene\": \"field\",\n  \"width\": 480,\n  \"height\": 270,\n  \
             \"fps\": 60,\n  \"frames\": 2,\n  \"ticks\": 2,\n  \
             \"world_hash\": \"0000000000000abc\",\n  \"mean_sprites\": 15.0,\n  \
             \"most_sprites\": 20,\n  \"mean_batches\": 2.0,\n  \"mean_tiles\": 600.0,\n  \
             \"mean_tick_ms\": 0.250,\n  \"longest_tick_ms\": 0.250,\n  \
             \"renderer\": \"llvmpipe \\\"x\\\\y\\\"\"\n}\n"
        );
        assert_eq!(quoted("a\u{1}"), "\"a\\u0001\"");
    }
}
