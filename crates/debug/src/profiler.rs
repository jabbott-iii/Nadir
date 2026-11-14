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

//! The profiler: the last frames' timings and scopes in a ring, their
//! graph and a frame's scope tree in a window, and the Chrome trace of
//! them (`debug-tools.md`, "Profiler").

use std::collections::VecDeque;
use std::fmt::{self, Write as _};
use std::ops::Range;
use std::time::{Duration, Instant};

use nadir_codec::json::{self, Value};
use nadir_core::profile::ScopeRecord;

use crate::{Plot, Ui};

/// The frames a [`Profiler`] keeps: five seconds at 60 frames a second.
pub const PROFILER_FRAMES: usize = 300;

/// The scope records a [`Profiler`] keeps, of all its frames together: a
/// frame of many records leaves room for fewer frames, and one of more
/// than this keeps this many.
pub const PROFILER_RECORDS: usize = 32_768;

/// The frames whose lengths [`Profiler::frame_rate`] averages.
const RATE_FRAMES: usize = 60;

/// The top of the graph: two frames at 60 a second. A frame that took
/// longer is drawn at the top.
const GRAPH_TOP_MS: f32 = 1000.0 / 30.0;

/// The graph's height, in UI units.
const GRAPH_HEIGHT: f32 = 60.0;

/// The trace's process and thread.
const PID: i64 = 1;
const TID: i64 = 1;

/// When a frame started, and how long it took.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameTiming {
    /// When the frame started.
    pub start: Instant,
    /// How long it was: until the next frame started.
    pub length: Duration,
    /// How long the processor worked on it: its length less the waits,
    /// such as the frame limiter's sleep and the display's swap.
    pub work: Duration,
}

impl FrameTiming {
    /// A frame that started at `start`, `length` long, `work` of which the
    /// processor worked on it.
    pub fn new(start: Instant, length: Duration, work: Duration) -> Self {
        Self {
            start,
            length,
            work,
        }
    }
}

/// A frame the profiler holds.
#[non_exhaustive]
#[derive(Clone, Copy, Debug)]
pub struct ProfiledFrame<'a> {
    /// Its number: the frames pushed before it, paused or not.
    pub index: u64,
    /// When it started and how long it took.
    pub timing: FrameTiming,
    /// Its scopes, in the order they ended, so that a parent follows its
    /// children.
    pub records: &'a [ScopeRecord],
    /// Its scopes not kept: those past [`PROFILER_RECORDS`].
    pub dropped: usize,
}

/// What the profiler's window did this frame.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProfilerResponse {
    /// "Write trace" was clicked: the caller writes
    /// [`Profiler::trace_json`] to a file, and says where with
    /// [`Profiler::set_note`].
    pub write_trace: bool,
}

/// A frame in the ring: where its records are in the pool.
#[derive(Clone, Copy, Debug)]
struct Slot {
    index: u64,
    timing: FrameTiming,
    first: usize,
    len: usize,
    dropped: usize,
}

impl Slot {
    fn range(&self) -> Range<usize> {
        self.first..self.first + self.len
    }
}

/// The last [`PROFILER_FRAMES`] frames' timings and scopes, which
/// [`Profiler::show`] draws as a graph of each frame's processor time and,
/// for the frame clicked, its scopes as a tree, and [`Profiler::trace_json`]
/// writes as a Chrome trace.
///
/// The engine pushes each frame with the scopes that
/// `nadir_core::profile::take_frame` collected. The records of all the
/// frames share one pool of [`PROFILER_RECORDS`], made at the first push:
/// a push after that allocates nothing. While paused, it keeps what it
/// holds, and counts the frames and their rate on.
pub struct Profiler {
    slots: VecDeque<Slot>,
    /// The frames' records, each frame's in one run: empty until the first
    /// push.
    records: Vec<ScopeRecord>,
    /// Where the next frame's records go.
    head: usize,
    /// The next frame's number.
    next: u64,
    /// The last frames' lengths, for the frame rate.
    lengths: [Duration; RATE_FRAMES],
    rate_next: usize,
    rate_count: usize,
    paused: bool,
    selected: Option<u64>,
    /// The selected frame's records in the order they started, as the tree
    /// shows them, and the keys that put them in it.
    order: Vec<ScopeRecord>,
    keys: Vec<OrderKey>,
    note: String,
}

impl Default for Profiler {
    fn default() -> Self {
        Self::new()
    }
}

impl Profiler {
    /// A profiler holding no frame, not paused.
    pub fn new() -> Self {
        Self {
            slots: VecDeque::with_capacity(PROFILER_FRAMES),
            records: Vec::new(),
            head: 0,
            next: 0,
            lengths: [Duration::ZERO; RATE_FRAMES],
            rate_next: 0,
            rate_count: 0,
            paused: false,
            selected: None,
            order: Vec::new(),
            keys: Vec::new(),
            note: String::new(),
        }
    }

    /// Adds a frame, with its scopes, and drops the oldest frames to make
    /// room for it. While paused, it only counts the frame and its length.
    pub fn push(&mut self, timing: FrameTiming, records: &[ScopeRecord]) {
        self.lengths[self.rate_next] = timing.length;
        self.rate_next = (self.rate_next + 1) % RATE_FRAMES;
        self.rate_count = (self.rate_count + 1).min(RATE_FRAMES);
        let index = self.next;
        self.next += 1;
        if self.paused {
            return;
        }
        if self.records.is_empty() {
            let blank = ScopeRecord {
                name: "",
                start: timing.start,
                duration: Duration::ZERO,
                depth: 0,
            };
            self.records = vec![blank; PROFILER_RECORDS];
        }
        let len = records.len().min(PROFILER_RECORDS);
        let first = if self.head + len > PROFILER_RECORDS {
            0
        } else {
            self.head
        };
        let range = first..first + len;
        while self.slots.len() == PROFILER_FRAMES
            || self
                .slots
                .iter()
                .any(|slot| overlaps(&slot.range(), &range))
        {
            self.slots.pop_front();
        }
        self.records[range].copy_from_slice(&records[..len]);
        self.head = first + len;
        self.slots.push_back(Slot {
            index,
            timing,
            first,
            len,
            dropped: records.len() - len,
        });
    }

    /// How many frames it holds.
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// Whether it holds no frame.
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// The frames it holds, the oldest first.
    pub fn frames(&self) -> impl ExactSizeIterator<Item = ProfiledFrame<'_>> + '_ {
        self.slots.iter().map(|slot| self.view(slot))
    }

    /// The frame numbered `index`, if it holds it.
    pub fn frame(&self, index: u64) -> Option<ProfiledFrame<'_>> {
        self.slot(index).map(|slot| self.view(slot))
    }

    /// The frames a second, over the last 60 frames pushed, paused or not:
    /// 0 before the first, or while they take no time.
    pub fn frame_rate(&self) -> f32 {
        let total: Duration = self.lengths[..self.rate_count].iter().sum();
        if total.is_zero() {
            0.0
        } else {
            self.rate_count as f32 / total.as_secs_f32()
        }
    }

    /// Whether it is paused: holding its frames while the game runs on.
    pub fn paused(&self) -> bool {
        self.paused
    }

    /// Pauses it, or lets it record again.
    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    /// The number of the frame whose scopes the window shows, if any.
    pub fn selected(&self) -> Option<u64> {
        self.selected
    }

    /// Shows the scopes of the frame numbered `index`, or of none.
    pub fn select(&mut self, index: Option<u64>) {
        self.selected = index;
    }

    /// Sets the line the window shows under its buttons, such as where a
    /// trace was written; an empty one shows nothing.
    pub fn set_note(&mut self, note: impl fmt::Display) {
        self.note.clear();
        // Writing to a `String` cannot fail.
        let _ = write!(self.note, "{note}");
    }

    /// Fills a window: a checkbox that pauses it, a "Write trace" button,
    /// the note, a graph of each frame's processor time from 0 to 33 ms
    /// (the oldest on the left), which says the frame under the pointer
    /// and selects the one clicked, and the selected frame's scopes as a
    /// tree, each with how long it took.
    pub fn show(&mut self, ui: &mut Ui<'_>) -> ProfilerResponse {
        let mut response = ProfilerResponse::default();
        ui.horizontal(|ui| {
            ui.checkbox("Paused", &mut self.paused);
            response.write_trace = ui.button("Write trace").clicked;
        });
        if !self.note.is_empty() {
            ui.label(&self.note);
        }
        let selected = self
            .selected
            .and_then(|index| self.slots.iter().position(|slot| slot.index == index));
        let plot = Plot {
            range: 0.0..=GRAPH_TOP_MS,
            height: GRAPH_HEIGHT,
            selected,
        };
        let values = self.slots.iter().map(|slot| millis(slot.timing.work));
        let graph = ui.plot("CPU ms##graph", values, plot);
        if let Some(slot) = graph.clicked.and_then(|at| self.slots.get(at)) {
            self.selected = Some(slot.index);
        }
        match graph.hovered.and_then(|at| self.slots.get(at)) {
            Some(slot) => ui.label(format_args!(
                "frame {}: {:.2} ms CPU, {:.2} ms long",
                slot.index,
                millis(slot.timing.work),
                millis(slot.timing.length)
            )),
            None => ui.label(format_args!(
                "{} frames, {:.0} a second",
                self.slots.len(),
                self.frame_rate()
            )),
        }
        ui.separator();
        let Some(index) = self.selected else {
            ui.label("Click a frame for its scopes.");
            return response;
        };
        let Some(&slot) = self.slot(index) else {
            ui.label(format_args!("Frame {index} is gone: click another."));
            return response;
        };
        ui.label(format_args!(
            "frame {}: {} scopes, {:.3} ms CPU",
            slot.index,
            slot.len,
            millis(slot.timing.work)
        ));
        if slot.dropped > 0 {
            ui.label(format_args!("{} more scopes not kept", slot.dropped));
        }
        start_order(&self.records[slot.range()], &mut self.keys, &mut self.order);
        tree(ui, &self.order);
        response
    }

    /// The frames it holds as Chrome trace event JSON, which Perfetto UI
    /// and `chrome://tracing` open: `{"traceEvents": [...]}`, a name for
    /// the thread, then for each frame a complete event (`"ph": "X"`)
    /// named `frame`, with its number and processor time as arguments,
    /// and one for each of its scopes, in the order they started, a parent
    /// before its children.
    ///
    /// Times are whole microseconds from the oldest frame's start, each
    /// start and end rounded down, so that, for a reader that converts
    /// them to nanoseconds, a scope stays inside its parent's event, two
    /// in a row never overlap, and a frame ends where the next starts,
    /// exactly. A scope shorter than a microsecond may become an instant,
    /// which at its parent's end a reader may show beside it. A scope that
    /// started before the oldest frame starts at 0, and ends where it
    /// ended.
    pub fn trace_json(&self) -> String {
        let origin = self.slots.front().map(|slot| slot.timing.start);
        let since =
            |at: Instant| origin.map_or(0, |origin| micros(at.saturating_duration_since(origin)));
        let span = |start: Instant, duration: Duration| {
            let ts = since(start);
            (ts, since(start + duration) - ts)
        };
        let mut events = Vec::with_capacity(1 + self.slots.len() + self.records_held());
        let (mut keys, mut ordered) = (Vec::new(), Vec::new());
        events.push(object(vec![
            ("name", Value::String("thread_name".to_string())),
            ("ph", Value::String("M".to_string())),
            ("pid", Value::Int(PID)),
            ("tid", Value::Int(TID)),
            (
                "args",
                object(vec![("name", Value::String("main".to_string()))]),
            ),
        ]));
        for frame in self.frames() {
            let index = i64::try_from(frame.index).unwrap_or(i64::MAX);
            let cpu_ms = micros(frame.timing.work) as f64 / 1000.0;
            let args = object(vec![
                ("index", Value::Int(index)),
                ("cpu_ms", Value::Float(cpu_ms)),
            ]);
            let (ts, dur) = span(frame.timing.start, frame.timing.length);
            events.push(complete("frame", ts, dur, Some(args)));
            start_order(frame.records, &mut keys, &mut ordered);
            for record in &ordered {
                let (ts, dur) = span(record.start, record.duration);
                events.push(complete(record.name, ts, dur, None));
            }
        }
        json::to_string_compact(&object(vec![("traceEvents", Value::Array(events))]))
    }

    fn records_held(&self) -> usize {
        self.slots.iter().map(|slot| slot.len).sum()
    }

    fn slot(&self, index: u64) -> Option<&Slot> {
        self.slots.iter().find(|slot| slot.index == index)
    }

    fn view(&self, slot: &Slot) -> ProfiledFrame<'_> {
        ProfiledFrame {
            index: slot.index,
            timing: slot.timing,
            records: &self.records[slot.range()],
            dropped: slot.dropped,
        }
    }
}

/// Shows `records`, sorted by when they started, as a tree: each a node
/// whose children are the records after it that are deeper, up to the
/// next one that is not; a record with no children a line.
fn tree(ui: &mut Ui<'_>, records: &[ScopeRecord]) {
    let mut at = 0;
    let mut position = 0_usize;
    while let Some(record) = records.get(at) {
        let deeper = records[at + 1..]
            .iter()
            .take_while(|child| child.depth > record.depth)
            .count();
        let children = &records[at + 1..at + 1 + deeper];
        let ms = f64::from(millis(record.duration));
        if children.is_empty() {
            ui.label(format_args!("  {} {ms:.3} ms", record.name));
        } else {
            // Its place among its siblings keeps a node open from frame to
            // frame, and apart from a sibling of its name.
            ui.tree_node(
                format_args!("{} {ms:.3} ms##{position}:{}", record.name, record.name),
                |ui| tree(ui, children),
            );
        }
        at += 1 + deeper;
        position += 1;
    }
}

/// Where a record's run of descendants begins in the order they ended, its
/// depth, and its own place there.
type OrderKey = (u32, u32, u32);

/// Puts `records`, in the order they ended, into `out` in the order they
/// started: each parent before its children, siblings in their order.
///
/// In the order they ended, a record follows its descendants, which are
/// the run of deeper records just before it. So the record before a run is
/// the root of a child's, whose own run begins where its key says: each
/// record finds where its run begins by stepping back over its children.
/// Sorted by that, then by depth, a parent comes before its first child,
/// and a sibling after the runs before it. Start times would not do: two
/// scopes may start on the same tick of a coarse clock (100 ns on
/// Windows), and no tie-break orders both a parent and a sibling right.
/// `keys` is a buffer the calls reuse.
fn start_order(records: &[ScopeRecord], keys: &mut Vec<OrderKey>, out: &mut Vec<ScopeRecord>) {
    keys.clear();
    for (index, record) in records.iter().enumerate() {
        let mut first = index;
        while first > 0 && records[first - 1].depth > record.depth {
            // `first - 1` ends a child's run: step back over all of it.
            first = keys[first - 1].0 as usize;
        }
        // A frame holds at most `PROFILER_RECORDS` records, so these fit.
        keys.push((first as u32, record.depth, index as u32));
    }
    keys.sort_unstable();
    out.clear();
    out.extend(keys.iter().map(|&(_, _, index)| records[index as usize]));
}

fn overlaps(a: &Range<usize>, b: &Range<usize>) -> bool {
    a.start < b.end && b.start < a.end
}

fn millis(duration: Duration) -> f32 {
    duration.as_secs_f32() * 1000.0
}

/// Whole microseconds, rounded down.
fn micros(duration: Duration) -> i64 {
    i64::try_from(duration.as_micros()).unwrap_or(i64::MAX)
}

fn object(members: Vec<(&str, Value)>) -> Value {
    Value::Object(
        members
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect(),
    )
}

/// A complete event (`"ph": "X"`) on the main thread.
fn complete(name: &str, ts: i64, dur: i64, args: Option<Value>) -> Value {
    let mut members = vec![
        ("name", Value::String(name.to_string())),
        ("ph", Value::String("X".to_string())),
        ("ts", Value::Int(ts)),
        ("dur", Value::Int(dur)),
        ("pid", Value::Int(PID)),
        ("tid", Value::Int(TID)),
    ];
    if let Some(args) = args {
        members.push(("args", args));
    }
    object(members)
}

#[cfg(test)]
mod tests {
    use nadir_codec::Limits;

    use super::*;

    const MS: Duration = Duration::from_millis(1);
    const US: Duration = Duration::from_micros(1);

    fn record(name: &'static str, start: Instant, duration: Duration, depth: u32) -> ScopeRecord {
        ScopeRecord {
            name,
            start,
            duration,
            depth,
        }
    }

    /// A frame at `start` of a tick with two systems, then a draw, as the
    /// recorder gives them: in the order they ended.
    fn frame_records(start: Instant) -> Vec<ScopeRecord> {
        vec![
            record("player_move", start + 10 * US, 100 * US, 1),
            record("movement", start + 120 * US, 300 * US, 1),
            record("nadir::tick", start + 5 * US, 500 * US, 0),
            record("nadir::render", start + 600 * US, 2 * MS, 0),
        ]
    }

    fn timing(start: Instant) -> FrameTiming {
        FrameTiming::new(start, 16 * MS, 3 * MS)
    }

    fn records_of(frame: &ProfiledFrame<'_>) -> Vec<&'static str> {
        frame.records.iter().map(|record| record.name).collect()
    }

    #[test]
    fn frames_keep_their_records_and_numbers_up_to_300() {
        let mut profiler = Profiler::new();
        assert!(profiler.is_empty());
        assert_eq!(profiler.frame_rate(), 0.0);
        let base = Instant::now();
        for index in 0..PROFILER_FRAMES as u32 + 20 {
            let start = base + index * 16 * MS;
            profiler.push(timing(start), &frame_records(start));
        }
        assert_eq!(profiler.len(), PROFILER_FRAMES);
        let indices: Vec<u64> = profiler.frames().map(|frame| frame.index).collect();
        assert_eq!(indices, (20..320).collect::<Vec<u64>>());
        let frame = profiler.frame(25).expect("frame 25 is held");
        assert_eq!(frame.timing.start, base + 25 * 16 * MS);
        assert_eq!(
            records_of(&frame),
            ["player_move", "movement", "nadir::tick", "nadir::render"]
        );
        assert_eq!(frame.records[0].start, base + 25 * 16 * MS + 10 * US);
        assert!(profiler.frame(19).is_none());
        // 60 frames of 16 ms each.
        assert!((profiler.frame_rate() - 62.5).abs() < 0.01);
    }

    #[test]
    fn a_frame_of_many_records_drops_the_oldest_frames_and_one_too_many_is_cut() {
        let mut profiler = Profiler::new();
        let start = Instant::now();
        let small = frame_records(start);
        for _ in 0..10 {
            profiler.push(timing(start), &small);
        }
        // Half the pool, twice: the second wraps to the pool's start, over
        // the small frames and the first big one.
        let big = vec![record("big", start, US, 0); PROFILER_RECORDS / 2 + 1];
        profiler.push(timing(start), &big);
        assert_eq!(profiler.len(), 11);
        profiler.push(timing(start), &big);
        let held: Vec<(u64, usize)> = profiler
            .frames()
            .map(|frame| (frame.index, frame.records.len()))
            .collect();
        assert_eq!(held, [(11, PROFILER_RECORDS / 2 + 1)]);

        let huge = vec![record("huge", start, US, 0); PROFILER_RECORDS + 5];
        profiler.push(timing(start), &huge);
        let frame = profiler.frame(12).expect("the huge frame is held");
        assert_eq!(frame.records.len(), PROFILER_RECORDS);
        assert_eq!(frame.dropped, 5);
        assert_eq!(profiler.len(), 1);
        profiler.push(timing(start), &small);
        assert_eq!(profiler.len(), 1, "the huge frame filled the pool");
        assert_eq!(records_of(&profiler.frame(13).expect("held")).len(), 4);
    }

    /// The names and depths of `records` in the order they started.
    fn started(records: &[ScopeRecord]) -> Vec<(&'static str, u32)> {
        let (mut keys, mut out) = (Vec::new(), Vec::new());
        start_order(records, &mut keys, &mut out);
        out.iter()
            .map(|record| (record.name, record.depth))
            .collect()
    }

    #[test]
    fn scopes_are_put_in_start_order_by_their_nesting_not_their_clock() {
        let at = Instant::now() + MS;
        // As the recorder gives them, in the order they ended.
        assert_eq!(
            started(&frame_records(at)),
            [
                ("nadir::tick", 0),
                ("player_move", 1),
                ("movement", 1),
                ("nadir::render", 0)
            ]
        );
        // Siblings started on one tick of the clock: `x` ended before `y`
        // began, so `z` is `y`'s, though `y` is the longer.
        let tie = [
            record("x", at, Duration::ZERO, 0),
            record("z", at, US, 1),
            record("y", at, 5 * US, 0),
        ];
        assert_eq!(started(&tie), [("x", 0), ("y", 0), ("z", 1)]);
        // A last child and the next sibling of its parent started on one
        // tick: `c` is `p`'s.
        let tie = [
            record("c", at, Duration::ZERO, 1),
            record("p", at - 5 * US, 5 * US, 0),
            record("q", at, 3 * US, 0),
        ];
        assert_eq!(started(&tie), [("p", 0), ("c", 1), ("q", 0)]);
        // Deep nesting, a parent of two children with their own, and the
        // children of a scope still open when the frame was collected,
        // whose parent is in the next frame.
        let nested = [
            record("a1", at, US, 3),
            record("a", at, US, 2),
            record("b", at, US, 2),
            record("p", at, US, 1),
            record("g", at, US, 0),
            record("orphan", at, US, 1),
        ];
        assert_eq!(
            started(&nested),
            [
                ("g", 0),
                ("p", 1),
                ("a", 2),
                ("a1", 3),
                ("b", 2),
                ("orphan", 1)
            ]
        );
        assert!(started(&[]).is_empty());
    }

    #[test]
    fn a_paused_profiler_holds_its_frames_and_counts_on() {
        let mut profiler = Profiler::new();
        let start = Instant::now();
        profiler.push(timing(start), &frame_records(start));
        profiler.set_paused(true);
        assert!(profiler.paused());
        for _ in 0..5 {
            profiler.push(FrameTiming::new(start, 8 * MS, MS), &frame_records(start));
        }
        assert_eq!(profiler.len(), 1);
        profiler.set_paused(false);
        profiler.push(timing(start), &[]);
        let indices: Vec<u64> = profiler.frames().map(|frame| frame.index).collect();
        assert_eq!(indices, [0, 6]);
        // The frames while paused count in the rate: 16, 8 × 5, 16 ms.
        let expected = 7.0 / 0.072;
        assert!((profiler.frame_rate() - expected).abs() < 0.01);
    }

    #[test]
    fn steady_pushes_allocate_nothing() {
        let mut profiler = Profiler::new();
        let start = Instant::now();
        let records = frame_records(start);
        profiler.push(timing(start), &records);
        let pool = profiler.records.as_ptr();
        for _ in 0..PROFILER_FRAMES * 3 {
            profiler.push(timing(start), &records);
        }
        assert_eq!(profiler.records.as_ptr(), pool);
        assert_eq!(profiler.records.len(), PROFILER_RECORDS);
        assert!(profiler.slots.capacity() >= PROFILER_FRAMES);
        assert_eq!(profiler.len(), PROFILER_FRAMES);
    }

    /// An event of the parsed trace: its name, and when it starts and ends.
    #[derive(Debug)]
    struct Event {
        name: String,
        start: i64,
        end: i64,
    }

    fn events(text: &str) -> (Value, Vec<Event>) {
        let value = json::parse(text, Limits::default()).expect("the trace parses");
        let list = value
            .get("traceEvents")
            .and_then(Value::as_array)
            .expect("traceEvents is an array")
            .to_vec();
        let mut complete = Vec::new();
        for event in &list {
            let field = |key: &str| event.get(key).expect(key);
            assert_eq!(field("pid").as_i64(), Some(PID));
            assert_eq!(field("tid").as_i64(), Some(TID));
            if field("ph").as_str() == Some("X") {
                let start = field("ts").as_i64().expect("ts is a whole number");
                let dur = field("dur").as_i64().expect("dur is a whole number");
                complete.push(Event {
                    name: field("name").as_str().expect("a name").to_string(),
                    start,
                    end: start + dur,
                });
            }
        }
        (Value::Array(list), complete)
    }

    /// The depth of each event: how many events enclose it, each one wholly
    /// inside the one that encloses it, and none crossing another.
    fn depths(events: &[Event]) -> Vec<(String, usize)> {
        let mut sorted: Vec<&Event> = events.iter().collect();
        sorted.sort_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));
        let mut open: Vec<&Event> = Vec::new();
        let mut depths = Vec::new();
        for event in sorted {
            while open.last().is_some_and(|parent| parent.end <= event.start) {
                open.pop();
            }
            if let Some(parent) = open.last() {
                assert!(
                    event.end <= parent.end,
                    "{event:?} crosses the end of {parent:?}"
                );
            }
            depths.push((event.name.clone(), open.len()));
            open.push(event);
        }
        depths
    }

    #[test]
    fn the_trace_parses_with_every_frame_and_scope_nested() {
        let mut profiler = Profiler::new();
        let base = Instant::now();
        for index in 0..3 {
            let start = base + index * 16 * MS;
            profiler.push(timing(start), &frame_records(start));
        }
        let (all, complete) = events(&profiler.trace_json());
        let Value::Array(all) = all else {
            unreachable!()
        };
        // The thread's name, then 3 frames of 4 scopes each.
        assert_eq!(all.len(), 1 + 3 + 3 * 4);
        assert_eq!(all[0].get("ph").and_then(Value::as_str), Some("M"));
        assert_eq!(
            all[0]
                .get("args")
                .and_then(|args| args.get("name"))
                .and_then(Value::as_str),
            Some("main")
        );
        assert_eq!(complete.len(), 3 + 3 * 4);
        // Each frame, then its scopes in the order they started.
        let first = &complete[..5];
        let names: Vec<&str> = first.iter().map(|event| event.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "frame",
                "nadir::tick",
                "player_move",
                "movement",
                "nadir::render"
            ]
        );
        assert_eq!((first[0].start, first[0].end), (0, 16_000));
        assert_eq!((first[1].start, first[1].end), (5, 505));
        assert_eq!((first[2].start, first[2].end), (10, 110));
        assert_eq!(complete[5].start, 16_000);
        assert_eq!(
            all[1]
                .get("args")
                .and_then(|args| args.get("index"))
                .and_then(Value::as_i64),
            Some(0)
        );

        let depths = depths(&complete);
        let expected = [
            ("frame", 0),
            ("nadir::tick", 1),
            ("player_move", 2),
            ("movement", 2),
            ("nadir::render", 1),
        ];
        for (index, chunk) in depths.chunks(5).enumerate() {
            let chunk: Vec<(&str, usize)> = chunk
                .iter()
                .map(|(name, depth)| (name.as_str(), *depth))
                .collect();
            assert_eq!(chunk, expected, "frame {index}");
        }
    }

    #[test]
    fn times_between_microseconds_keep_scopes_nested_and_frames_meeting() {
        const NS: Duration = Duration::from_nanos(1);
        // Frames of 16,666,667 ns; a parent from 0.4 to 1.6 µs into each,
        // and its child from 0.9 to 1.2 µs, both inside it.
        let length = 16_666_667 * NS;
        let mut profiler = Profiler::new();
        let base = Instant::now();
        for index in 0..4 {
            let start = base + index * length;
            let records = [
                record("child", start + 900 * NS, 300 * NS, 1),
                record("parent", start + 400 * NS, 1_200 * NS, 0),
            ];
            profiler.push(FrameTiming::new(start, length, MS), &records);
        }
        let (_, complete) = events(&profiler.trace_json());
        let frames: Vec<(i64, i64)> = complete
            .iter()
            .filter(|event| event.name == "frame")
            .map(|event| (event.start, event.end))
            .collect();
        assert_eq!(
            frames,
            [
                (0, 16_666),
                (16_666, 33_333),
                (33_333, 50_000),
                (50_000, 66_666)
            ]
        );
        // None crosses another's end.
        depths(&complete);
        // Each frame, then its parent and child, each inside the one before.
        for (index, chunk) in complete.chunks(3).enumerate() {
            let [frame, parent, child] = chunk else {
                panic!("three events a frame");
            };
            assert_eq!(
                (
                    frame.name.as_str(),
                    parent.name.as_str(),
                    child.name.as_str()
                ),
                ("frame", "parent", "child")
            );
            for (outer, inner) in [(frame, parent), (parent, child)] {
                assert!(
                    outer.start <= inner.start && inner.end <= outer.end,
                    "frame {index}: {inner:?} is not inside {outer:?}"
                );
            }
        }
        // In the third frame the child, shorter than a microsecond, ends
        // up at the end of its parent, as an instant.
        assert_eq!((complete[7].start, complete[7].end), (33_333, 33_334));
        assert_eq!((complete[8].start, complete[8].end), (33_334, 33_334));
    }

    #[test]
    fn an_empty_profiler_writes_a_trace_of_the_thread_name_alone() {
        let (all, complete) = events(&Profiler::new().trace_json());
        assert_eq!(all.as_array().map(<[Value]>::len), Some(1));
        assert!(complete.is_empty());
    }

    #[test]
    fn names_are_escaped_and_a_scope_before_the_first_frame_starts_at_0() {
        let mut profiler = Profiler::new();
        let base = Instant::now() + MS;
        let records = [record("say \"hi\"\n", base - MS, 2 * MS, 0)];
        profiler.push(timing(base), &records);
        let (_, complete) = events(&profiler.trace_json());
        assert_eq!(complete[1].name, "say \"hi\"\n");
        // It started 1 ms before the oldest frame, and ran for 2 ms.
        assert_eq!((complete[1].start, complete[1].end), (0, 1000));
    }
}
