# Subsystem: `nadir-debug` (feature `dev` only)

In-game developer tools, built entirely on our own renderer, bitmap font and
input. None of it ships: `cargo xtask release-no-dev` verifies that.

## Immediate-mode UI

- **Model:**
  - Each frame, UI code calls widget functions. Each widget returns its
    interaction result (`clicked`, `changed`).
  - Widget identity is a hash of (parent ID, label or explicit ID).
  - State kept across frames: `hot` (hovered) and `active` (pressed)
    widget, window positions, open/closed trees, text-edit cursor.
- **Layout:** vertical stack by default, with `horizontal(|ui| …)` rows.
  Fixed-height rows sized from font metrics.
- **Windows:**
  - draggable title bar, collapse toggle, resizable from the bottom-right
    corner, vertical scrolling via clip rect + offset
  - positions persist to `debug_ui.jsonc` in the user config directory
- **Widgets:**
  - `label`, `button`, `checkbox`, `slider_f32`/`slider_i32`, `drag_f32`
  - `text_input` (single line, cursor, selection with shift+arrows, copy and
    paste deferred)
  - `combo`, `tree_node`, `separator`, `plot` (line graph of a ring buffer)
  - `color_edit` (RGBA sliders)
- **Rendering:** UI sprites and text go into `RenderFrame.ui_sprites` /
  `texts` at native resolution, scaled by the UI scale setting, after the
  game UI.
  - As built in M7.8 (ADR-0055), the renderer's UI is `RenderFrame::ui`,
    a `UiList` drawn in the order pushed: the debug UI sets its own view
    (`UiView::new(UiScale::Free(setting))`), and pushes after the game's
    UI, which puts it on top; a window's clip is `UiView::clipped`, which
    intersects for a scrolled region inside it. `UiSpace::to_ui` maps the
    pointer, with no GPU.
- **Input capture:** when the pointer is over any debug window, or a debug
  text field has focus, the UI reports `wants_mouse`/`wants_keyboard` and the
  facade suppresses gameplay actions.

### As built (M7.2)

The UI core is nadir-debug's alone (ADR-0056): the engine feeds it, draws
it, holds the game's input off and saves its windows' places in M7.10. The
window's typed text and the keys' repeats reach it through
`nadir_input::UiEvents`, and `nadir_input::Input::set_capture` holds the
game's actions off (M7.9, ADR-0057). It depends on core and render.

```rust
pub struct DebugInput;   // pointer_moved, button, wheel, key(key, pressed, repeat), text, focus_lost, clear
pub trait TextMetrics { fn line_height(&self) -> f32; fn caret_x(&self, text: &str, byte: usize) -> f32; }
pub struct FontMetrics;  // FontMetrics::new(font: Font): TextMetrics of a copy of the font
pub struct DebugUi;      // new(font, metrics, white), set_font, style, style_mut, frame,
                         // wants_mouse, is_over (M7.10), wants_keyboard, places,
                         // set_place, places_revision
pub struct Frame<'a>;    // window(title, first: Rect, add) -> bool, finish(&mut UiList)
pub struct Ui<'a>;       // with_id, horizontal, key_pressed, focus_next, label, label_colored,
                         // button, checkbox, slider_f32, slider_i32, drag_f32, text_input,
                         // combo, tree_node, separator, plot, color_edit
pub struct Response { hovered, active, clicked, changed, submitted, lost_focus }  // non_exhaustive
pub struct Plot { range, height, selected }; pub struct PlotResponse { response, hovered, clicked }
pub struct WindowPlace<'a> { title, rect, collapsed }; pub struct Style { scale, ... }
```

- **Input:** `DebugInput` keeps the window's events in order; each
  `DebugUi::frame` takes at most one press and one release of the left
  button, so that a press is judged where it happened and a click where
  the release happened, at any frame rate. A frame ends at its release,
  so a drag ends where the button went up; a press after a release, or
  after text or keys, waits for the next frame, so that what was typed
  goes to the field that had the keyboard. Text and keys are one ordered
  list of edits; a key repeated is pressed again; control characters are
  left out of text. `focus_lost` lets go with no click; `clear` drops
  what was not taken, for a debug UI drawn again after a while. The
  events are taken at `frame`, so a frame dropped unfinished replays
  nothing.
- **Units:** the UI's units at `Style::scale`, by default
  `UiScale::Free(1.0)`; `frame` makes the `UiSpace` (render's) from the
  output and virtual sizes and maps the pointer.
- **Identity:** a widget's is the hash of its scope's (its window, a tree
  node, a color edit, `with_id`) and its text, or its explicit ID after
  `##`: `"Pause (3)##pause"` shows `Pause (3)`, and is `pause`. Windows
  too: a window's place is named by its explicit ID or its title.
- **Hot, active, focus:** hit tests use the windows drawn in the last
  frame, front first, a combo's list before them, collapsed windows by
  their title bar. A press makes the widget under it active until the
  release; a click is a release over it. A text field clicked has the
  keyboard until Escape, a press elsewhere or another field takes it, which
  the field hears (`lost_focus`) when next called; Enter is `submitted`
  and keeps it; `Ui::focus_next` gives it to the next field, which takes
  none of that frame's edits. A held widget, a focused field or an open
  list that is not called in a frame lets go.
- **Capture:** `wants_mouse` (the pointer over a window drawn, or a widget
  held) and `wants_keyboard` (a field drawn with the keyboard). A press
  on a window is the UI's until its release, and one outside every window
  the game's.
- **Windows:** a title bar that drags (kept at least partly on screen),
  a collapse box, a grip at the bottom-right that resizes (to
  `Style::min_size`), and, for content taller than the window, the wheel
  and a scrollbar's thumb. Chrome is hit before content. A drag starts
  from where the window is drawn, its place kept on screen. A press brings
  a window to the front; one shown again while others are drawn comes to
  the front. A second call of a window in a frame goes on from the first,
  laid out as it was.
- **Layout:** one row a widget, a line high plus padding; `horizontal`
  rows; a control takes `control_share` of a row (its label after it), or
  `control_width` in a horizontal row; tree children move in by `indent`.
  A label out of view in a column is not formatted.
- **Widgets:** buttons click on release only after a press on them;
  sliders hold their value in the range (exact at its ends, a reversed
  range swapped, a NaN bound inert, a value out of range kept until moved,
  `i32` over its whole range); `drag_f32` ignores a step or result that is
  not finite; the text field edits one line (Backspace, Delete, Left,
  Right, Home, End, Shift to select, Control or Super with A, no Alt);
  `combo`'s list opens over everything and closes on a pick or a press
  elsewhere; `plot` draws a line graph, says the value under the pointer
  and the one clicked, and marks one.
- **Places:** `places` gives every window's place as the user or
  `set_place` left it (clamped on screen only when drawn);
  `places_revision` changes when a move, a resize or a collapse ends.
- **Drawing:** `Frame::finish` pushes each window from back to front, a
  combo's list last, into the `UiList`, in layer 255 at
  `SortMode::Fixed(u32::MAX)`, the view of each its window's clip at the
  style's scale, text with no markup; the list's view is put back.
- **Not built yet:** a scroll area inside a window, rows called back only
  when in view, tooltips, window options (no title bar, a close box),
  wider integers, copy and paste. They are additive, for the tools that
  need them (M7.3 to M7.5). The golden of a debug window is M7.10's.

### As built (M7.10): the debug UI in the game

The facade shows the debug UI in development builds (feature `dev`;
ADR-0058), and re-exports this crate as `nadir::debug`.

```rust
// nadir_engine, with `dev`:
pub type DebugWindow = fn(&World, &mut nadir::debug::Ui<'_>);
impl App { pub fn add_debug_window(self, toggle: Key, title: &'static str, first: Rect,
                                   window: DebugWindow) -> Self; }
pub struct WindowSize(pub UVec2);   // a resource: the window's inside, physical pixels
```

- **Windows:** a game adds its own, each shown and hidden by a key of its
  own (a press, not a repeat, whether a field has the keyboard or not),
  hidden at start; they are filled in the order added, after the frame
  stages, inside `App::update`, so over the game's UI. The sandbox's is on
  F8 (`cargo dev`): a note to type, what the last frame drew, every
  entity's place. The tools below will be windows of the engine's, on
  their keys.
- **The UI** is made the first time a window is open, in the engine's
  font, and then runs every frame; before that its input is fed and
  cleared. It reads the window's events from a `nadir_input::UiEvents`,
  in order, cleared every frame.
- **The font:** a 5 × 7 pixel font of printable ASCII, monospaced, 6
  pixels a character and 10 a line, in the facade's source. With `dev` the
  asset server's source makes its cooked files, and a white texel, in
  memory, at `nadir/debug/` (`font.font.kbj`, `font.kbj`, `font_0.qoi`,
  `white.qoi`), before the game's loose files. Other characters draw a
  box.
- **Scale:** `UiScale::Free` of the window's scale factor, rounded to a
  whole number and at least 1, until the settings file's UI scale (M9.3).
- **Capture:** the keys at each frame's start, from the UI's last
  `wants_keyboard`. The mouse at each press of a button and each turn of
  the wheel, before it reaches `Input`: the UI's while it holds the left
  button from a press on a window, the game's while the game holds it,
  and otherwise the UI's if the pointer `is_over` a window drawn in the
  frame before; a press of the left button decides who holds it, until
  its release or a loss of the focus. Never by where the pointer rests,
  so a button the game holds stays the game's as the pointer passes a
  window. The facade sets `Input::set_capture` only when that changes. A
  replay plays on.
- **Places:** `App::run` alone reads `debug_ui.jsonc` in the user config
  directory before the loop, and writes it, through a temporary file,
  each time `places_revision` changes: version 1, a comment line, then
  `{ "version": 1, "windows": [ { "name", "x", "y", "w", "h",
  "collapsed" } ] }`. A file that cannot be read is a warning; a window
  not whole in it is left out; other members are ignored.
- **The golden:** `debug_window`, the facade's (`docs/04-quality.md`).

### As built (M7.3): the stats overlay and the profiler

Two windows of the engine's, in the facade's debug UI (ADR-0059). The GL
timer queries are M7.11's (ADR-0060), and the profiler's GPU time
M7.12's (ADR-0061, below).

```rust
// nadir-debug:
#[non_exhaustive] pub struct Stats { frame_rate, frame_time, tick_time, ticks, draw_calls,
                                     sprites, entities, allocations, allocated_bytes,
                                     voices, underruns }       // Default; show(&self, ui)
pub const PROFILER_FRAMES: usize = 300;
pub const PROFILER_RECORDS: usize = 32_768;
#[non_exhaustive] pub struct FrameTiming { pub start: Instant, pub length: Duration, pub work: Duration }
#[non_exhaustive] pub struct ProfiledFrame<'a> { pub index: u64, pub timing: FrameTiming,
                                                 pub records: &'a [ScopeRecord], pub dropped: usize }
#[non_exhaustive] pub struct ProfilerResponse { pub write_trace: bool }
pub struct Profiler;   // new, push(timing, records), len, is_empty, frames, frame(index),
                       // frame_rate, paused, set_paused, selected, select, set_note,
                       // show(ui) -> ProfilerResponse, trace_json() -> String
```

- **The stats overlay** (F1, a window named `nadir::stats`): the frame
  rate; the last frame's processor time; this frame's ticks and how long
  they took (`TickStats`, set before the window is filled); the draw calls
  of the last frame drawn (`batches + chunks +
  ui_batches` of `FrameStats`: the blit onto the window is not counted);
  the world's sprites drawn; the entities alive (`World::entity_count`);
  the last frame's allocations and their bytes (`FrameAllocs`); the
  voices sounding and the underruns so far (`Audio::stats`), underruns in
  amber once there are some. nadir-debug takes plain numbers: the facade
  fills a `Stats` each frame. It is a window like any other, which drags
  and keeps its place: a window with no title bar that takes no input is
  not built.
- **Frames:** `App::update`, as it starts, collects the scopes recorded
  since the last frame started (`profile::take_frame`) and pushes the last
  frame to the profiler: when it started, how long until this one did,
  and how long the processor worked on it. A frame is then one update and
  its drawing. The processor's work is the update and, in `App::run`, the
  drawing and the hot reload, less the present, which may wait for the
  display; the frame limiter's sleep is outside it. `dev` turns on nadir-core's `profile`,
  so each system's scope is recorded (the schedule's), with the engine's:
  `nadir::tick` around each tick, `nadir::frame_stages`,
  `nadir::debug_ui`, and in `App::run` `nadir::render`, `nadir::present`
  and `nadir::finish_loads`. Only the main thread's scopes are collected.
- **The ring:** the last 300 frames, their records in one pool of 32,768,
  made at the first push; each frame's records are one run of the pool,
  and a frame that does not fit before the pool's end starts at its
  start, dropping the oldest frames in its way. A frame of more records
  than the pool keeps the first 32,768 and counts the rest as `dropped`.
  A push after the first allocates nothing. Paused, it keeps its frames
  and counts the frames and their rate on; `frame_rate` averages the last
  60 frames' lengths.
- **The window** (F3, `nadir::profiler`): "Paused", "Write trace", the
  note, a graph of each frame's processor time from 0 to 33 ms, the
  oldest on the left, which says the frame under the pointer and selects
  the one clicked; then the selected frame's scopes in the order they
  started, as a tree: a scope with children a node, by its place among its
  siblings and its name, so that a node stays open from frame to frame; a
  scope without a line. The order comes from the order they ended and
  their depths, which together give each scope its parent, not from their
  start times, which two scopes can share on a coarse clock (100 ns on
  Windows). A frame no longer held says so. Until the ring is
  full, the graph's lines grow the UI's buffers; then a frame of the open
  windows allocates nothing.
- **The trace:** `trace_json` writes, with nadir-codec's JSON writer,
  `{"traceEvents": [...]}`: a `thread_name` metadata event, then for each
  frame a complete event named `frame`, with `index` and `cpu_ms` as
  `args`, and one for each of its scopes in the order they started, a
  parent before its children, all on `pid` 1 and `tid` 1. Times are whole
  microseconds from the oldest frame's start, each start and end rounded
  down: a reader converting fractional microseconds to nanoseconds with
  floating point could make two frames that meet overlap by a nanosecond.
  So each scope lies inside its parent's event and each frame's scopes
  inside the frame's, two in a row never overlap, and frames meet
  exactly; a scope shorter than a microsecond may become an instant, which
  a reader may show beside its parent if it falls on the parent's end. A
  scope that started before the oldest frame starts at 0. "Write trace" makes the facade write it,
  from `App::run`, to `trace-<UTC time>.json` in the logs folder, beside
  `nadir.log`, and say where in the window's note and the log; elsewhere
  the note says traces are written only by `App::run`. The console's
  `profile dump [path]` is M7.5's.

### As built (M7.12): the GPU's time in the profiler

The renderer's timers (M7.11) feed the profiler in development builds
(ADR-0061).

```rust
// nadir-debug:
#[non_exhaustive] pub struct GpuTiming { pub submitted: Instant, pub world: Duration,
                                         pub output: Duration }   // new, total
// ProfiledFrame gains `pub gpu: Option<GpuTiming>`; Profiler gains
// next_index() -> u64 and set_gpu(index, GpuTiming) -> bool.
```

- **Timing:** the main loop of `App::run` turns the renderer's timing on
  under `dev`, before its first frame, on a device with timer queries;
  `App::offscreen` does not time.
- **Frames:** after each frame drawn, the facade gives the times that
  render read back (`Renderer::gpu_times`) to the frames they were timed
  in. It keeps the frame number (`Profiler::next_index` while the frame
  runs) and the drawing's start of the last 8 renders, by render, as a
  frame not drawn (a modal one) is no render: a time `renders_ago`
  renders back goes to that render's frame, with `Profiler::set_gpu`, and
  one 8 or more back is dropped. The profiler keeps it only for a frame
  it holds. The newest two or three frames wait for theirs; on llvmpipe
  the first has none (ADR-0060).
- **The window:** under the processor's graph, a graph of each frame's
  GPU time, both passes, on the same scale, 0 to 33 ms, and with the same
  frame marked; a frame without one is a gap. Either graph says the frame
  under the pointer, `frame 1157: 6.28 ms CPU, 4.49 ms GPU, 16.67 ms long`
  (without the GPU's part when it has none), and selects the one clicked.
  Under the selected frame's line, `GPU 4.366 ms: world 2.157, output
  2.209`, or `no GPU time`, then its tree.
- **The trace:** when a frame held has a GPU time, a second track, `tid`
  2, named `gpu`: for each frame with a time, an event `frame` (`args`:
  `index`, `gpu_ms`) over its passes, `world` then `output`. The timers
  give lengths, not when: a frame's passes start at the later of when its
  drawing started on the processor and when the frame before on the track
  ended, one after the other, as a GPU does one frame's work after
  another's. Their lengths are measured, their starts an estimate.
  Taking the timers' times as exact, a frame's start is never before its
  drawing started, nor after the GPU started its world pass's timer; a
  frame placed from its drawing's start is early by at least the time
  until the renderer began timing its world pass (in `cargo dev` on
  2026-11-14 the drawing started 10 to 72 µs before the `nadir::render`
  scope, and the passes are issued inside it), and one placed after the
  frame before by at least as much as that one, less up to a microsecond
  for the rounding (ADR-0061). Rounded down to microseconds as the rest
  is, the passes meet inside their frame and the track's frames never
  cross.
- **The graph's dots:** a frame with a GPU time between two without is
  a dot: `Ui::plot` draws a value with no finite one beside it as a dot,
  not only a plot's one value.

### As built (M7.4): the inspector

A window of the engine's, in the facade's debug UI, over a world's
entities and the scene's reflection (ADR-0062).

```rust
// nadir-debug (depends on nadir-ecs and nadir-scene since M7.4):
#[non_exhaustive] pub struct InspectorResponse { pub save_prefab: bool }
pub struct Inspector;  // new, selected, select, prefab_name, set_note,
                       // show(world, ui, cx) -> InspectorResponse,
                       // prefab(world, leave_out, cx) -> Option<String>
// nadir-ecs:   World::entities_into(&mut Vec<Entity>)
// nadir-scene: Registration::remove(commands, entity); Reflect::variants() -> &[&str]
```

- **The list:** a "Filter" field; the count, `340 entities` or `12 of 340
  entities`; then a page of 16 rows, each a button of the entity's
  number (`3v1`, its slot and generation), marked `>` when selected, and
  its registered components' names; `<`, `>` and `17-32 of 340` past one
  page. Each word of the filter must match the entity's number, or the
  name of a component it has, ignoring case. The entities come from
  `World::entities_into`, so one with no reflected component is listed
  too. A click selects; the selection takes effect in the frame clicked,
  its mark in the next.
- **The selected entity:** `Entity 3v1` and "Despawn"; the prefab's name
  and "Save as prefab"; a combo of the types it lacks, the data to make
  one from, and "Add"; the note;
  then a node for each registered component it has, in the registry's
  order, with its "Remove". A despawned one says `Entity 3v1 is gone:
  click another.`
- **Editors,** each through the component's `Reflect` impl: a struct's
  fields are rows (a struct in one, a node of its own; a tag says it has
  no fields); a list's elements rows under a node; an option its value,
  or `none`; an enum a combo of its `variants`. The rest is edited as its
  data, `to_value`: a boolean a checkbox; a float a drag, 0.1 a unit; a
  whole number (a `u64` past an `i64` too, which data writes as digits), a
  string or an asset's path a text field, applied on Enter or a press
  elsewhere, dropped on Escape or when its field is not drawn in a frame
  (the window closed or collapsed); a `Color` a color edit, as is a string
  that writes one (`#rrggbb`, `#rrggbbaa`) among the members of a value
  reflected whole (a `Sprite`'s `color`), whose types its data does not
  say; one to four numbers a row, the label after the last (a position's
  `[x, y]`), in a scope of its own; an object's members rows under a node,
  and a component's own (a `Sprite`'s) inline.
  A change is applied at once through `apply_value`: a member alone, in
  an object, which the facade's `Sprite` and `Collider` apply by member;
  a list whole. A value refused is not set, and the note says why:
  ``Not set: the inspector: at `Health.max`: 300 does not fit in a `u8` ``.
- **Structure** goes through the world's commands, so it shows after the
  next tick or frame: "Despawn"; "Add", a component made from the JSONC in
  the field beside it, `{}` at first, which a struct of defaults reads
  (`0` for a `SaveId`, `[0, 0]` for a `Velocity`, `"Down"` for a `Facing`,
  `{"texture": "atlas/sprites_0.qoi"}` for a `Sprite`; data the type
  refuses, or text that is not JSON, says why in the note and adds
  nothing); "Remove". A refusal, of an edit or of an "Add", is logged too
  (`warn!`), whole: the window cuts a long note at its edge.
- **Save as prefab:** with a name the prefab reader and the cook take,
  lowercase letters, digits and `_` (`nadir_codec::prefab::is_name`, 64
  bytes at most), `InspectorResponse::save_prefab`; `prefab` is
  the text: ``// The prefab `goblin`, saved by the inspector from entity
  3v1.``, then `{"name": ..., "components": {...}}`, pretty, each
  registered component the entity has but those `leave_out` names, as
  `to_value` writes it.
- **The facade** shows it on F2, `nadir::inspector`, first at (584, 8) to
  (944, 508), with `loading_context` in `Scope::Level` as its context.
  While it is open, the selected entity is outlined in the world, a unit
  outside it, in amber, in front of every sprite: its sprite as drawn
  (between the last two ticks, by its parent's chain, mirrored about its
  pivot, unrotated), else its collider's bounds, else a box 8 units wide at
  its place; nothing for an entity with no `Transform`. "Save as prefab"
  writes `prefabs/<name>.jsonc` in the logs folder `App::run` names,
  beside `nadir.log` and the traces, without `Transform` and `SaveId`,
  and says where in the note and the log; a name whose file in a game's
  assets, `data/prefabs/<name>.jsonc`, is no asset path (a Windows
  device's, `nul`) is not written, and the note says why.
- **Allocation:** a frame of the list alone allocates nothing once its
  buffers have grown; a component open builds its value each frame, and
  the facade's context its resource list, while the window is open.
- **Not built:** picking an entity in the world, setting an option that is
  `None`, a list's length or a `null`, scrolling the list past a page.

### As built (M7.5): the console

A window of the engine's, in the facade's debug UI, over the engine's log
(ADR-0063).

```rust
// nadir-debug:
pub const CONSOLE_LINES: usize = 1000;
pub const CONSOLE_HISTORY: usize = 100;
#[non_exhaustive] pub enum ConsoleError { BadUsage, UnclosedQuote, UnknownCommand,
    MissingArgument { usage, argument }, TooManyArguments { usage },
    InvalidArgument { usage, argument, found, expected }, Failed(String) }
pub struct Args<'a>;        // usage, len, is_empty, get, str, f32
pub struct ConsoleOut<'a>;  // print, print_colored
pub struct Console<C>;      // new, register(usage, help, command), line, set_line, lines,
                            // print, print_error, log_record, older, newer, complete,
                            // run_line(line, run), show(ui, run)
// run: FnMut(C, &Args, &mut ConsoleOut) -> Result<(), ConsoleError>
// nadir-core:   log::since(cursor, each: FnMut(&str)) -> u64
// nadir-assets: AssetServer::reload_all() -> usize
// nadir_engine, with `dev`:
pub type ConsoleCommand = fn(&World, &Args<'_>, &mut ConsoleOut<'_>) -> Result<(), ConsoleError>;
impl App { pub fn add_console_command(self, usage: &str, help: &str,
                                      command: ConsoleCommand) -> Self; }
```

- **The registry:** a command is registered by its usage, whose first word
  is its name, a line of help, and the caller's value for it (`C`: the
  facade's is an enum of the engine's commands and the game's functions);
  one of a name already there replaces it. A name is no placeholder, has
  no quote, and is not `help`, the console's own: `help` lists every
  command's usage and help in columns, `help <name>` one.
- **Lines:** words split at whitespace; a double quote groups words,
  spaces too (`profile dump "C:\My Traces\t.json"`), in which `\"` is a
  quote and `\\` a backslash, any other backslash kept; outside quotes a
  backslash is itself; `""` is an empty word; a quote not closed is an
  error. The first word names the command, matched as typed.
- **Usages** name each argument: a word to type as it is (`dump` of
  `profile dump [path]`), a value (`<x>`, or `[path]` if it may be left
  out), or a choice (`<error|warn|info|debug|trace>`). A line is checked
  before its command runs: each word typed, each choice one of its own,
  ignoring case and then spelled as the usage spells it, each required
  argument there, no more than the usage has. `Args::f32` reads a finite
  number. Each error names the argument and the usage: `<y> is missing:
  tp <x> <y>`, ``<scale>: `fast` is not a number: timescale <scale>``.
- **Completion:** Tab completes the line's last word, or the next after a
  space: the first from the names, `help`'s argument from the names, the
  others from their slots in the usage, a word or a choice's choices. One
  match goes in whole, a space after it; several as far as they agree, or,
  if they agree no further, are listed in the log, a match a line. A
  value (a prefab's or
  a level's name, a number) has no completions, nor a word in quotes.
- **History:** the last 100 lines run, one the same as the line before
  kept once; Up goes back, Down forward and, past the newest, to the line
  that was being typed.
- **The log:** the last 1000 lines: each line run after `>`, in the
  accent color; what commands print; errors in red; and the records the
  caller adds (`log_record`), `WARN` in the stats overlay's amber and
  `ERROR` in red, a record of several lines a line each. A line too long is
  cut at the window's edge.
- **The window** (`show`): as many of the log's newest lines as fit,
  bottom first, then the command line across the window, the rows sized
  whole so that the window does not scroll once its size is settled (a
  frame of a resize that shrinks it may show its scrollbar, as a window
  measures its content a frame late); only the lines in view are
  formatted. The command line takes the keyboard when the console is shown
  after a frame it was not, none of that frame's typing or keys reaching
  it. While the line has the keyboard: Enter runs it and empties it,
  keeping the keyboard; Tab, Up and Down, after the frame's typing;
  PageUp and PageDown a page, before the log is drawn. The wheel over the
  log scrolls three lines a line; a turn that is not finite scrolls that
  frame at most. A log scrolled up stays on its lines as new ones come,
  until a line is run, which brings it to its newest so that the line's
  output is in view (found in the check of `cargo dev`); one at its bottom
  shows the newest.
- **Input** (`DebugInput`, since M7.5): a frame's events end at a press of
  Enter (the keypad's too), Tab, Up, Down, PageUp or PageDown, the keys a
  text field leaves to its caller, so a caller handling them after the
  field sees each after the typing before it and before what follows:
  text typed after Enter goes to the next line, and Up then Enter runs the
  line recalled. A caller sees at most one of them in a frame.
- **The facade** shows it on the backtick (`Key::Grave`), `nadir::console`,
  first at (8, 316) to (944, 556). The character a window's toggle types
  as it is pressed, the text after its press and before another key's, is
  fed to no debug field, so the backtick that shows or hides the console
  is typed nowhere; a toggle on a key that types (a game's on a letter)
  cannot be typed into a debug field. While it is open, each frame gives it
  the log's records since it last read them (`log::since`, which holds the
  log's lock and allocates nothing), so it first shows what the ring holds.
  Its commands, after `help`:

  | Command | What it does |
  | :- | :- |
  | `schedule` | each system, in the order it runs (`Schedule::describe`) |
  | `spawn <prefab> [x] [y]` | loads the prefab into the `PrefabLibrary` if it is not there, in `Scope::Level`, and spawns it through the world's commands at (x, y), or where the camera looks; x and y go together |
  | `tp <x> <y>` | moves each entity with `Player` and a `Transform` there, its `PrevTransform` too |
  | `timescale <scale>` | 0.25 to 4: each frame's length, times the scale, is the clock's, so ticks, tiles and clips all follow; 1 at start |
  | `level <name>` | `load_level`, after the frame's debug UI, which says why one did not load; as `load_level` does, it takes the current level away first, so a name that does not load leaves no level (`level <the old name>` brings it back) |
  | `reload` | `AssetServer::reload_all`; the hot reload of `App::run` swaps each in and makes the level again |
  | `profile dump [path]` | the profiler's trace to `path`, its folder made, or beside the log as "Write trace" does |
  | `log level <target> <level>` | `log::set_target_level` of the target, a module path |
  | `quit` | ends `App::run`'s loop, as closing the window does |

  A game's commands (`App::add_console_command`) get the world, as a debug
  window does; one whose usage starts with no name is logged as an error
  and not added. Commands queue as a window's do: a spawn shows after the
  next tick or frame's stages.
- **Allocation:** a frame of the open console in which nothing is typed
  allocates nothing once its 1000 lines are there and no record is longer
  than the line it reuses; a line run, a Tab, the first Up, and typing
  that grows the line past its longest allocate (the line keeps its buffer
  across a run).
- **Not built:** completion of a value (a prefab's, a level's, a log
  target's name), the caret's place in completion (the last word is
  completed), a command typed with a backtick in it, copy and paste, the
  history kept across runs, `god` (left to games, below). On a layout where
  the key left of `1` is a dead key (German `^`, US-International `` ` ``),
  its accent comes with the next key, after the frame that opens the
  console, and is typed, or combined, into its line.

### As built (M7.6): debug drawing and time control

In the facade's debug UI, under `dev` (ADR-0064): F4's window, whose ticked
overlays draw into the world, and the time keys F5 to F7, a label in the
corner while the game's time is not real time.

```rust
// nadir-collision:
impl CollisionWorld { pub fn hash_cells(&self, each: impl FnMut(Rect, u32)); }
// nadir_engine, with `dev`: no public API; crates/engine/src/debug_ui/draw.rs
// (Overlays, the pen) and debug_ui/time.rs (Time, the keys, SCALES, TICK).
```

- **The window** (F4, `nadir::debug_draw`, first at (952, 8) to (1092,
  162), 128 before M9.5's two rows): a checkbox for each overlay, its name
  in its color. While the
  window is open, collapsed or not, the ticked overlays draw; closed, none
  do. At first: colliders, triggers, the tile grid and the paths.
- **The overlays,** each as the frame draws it, entities `alpha` of the
  way from the last tick's place, culled to the camera's view:

  | Overlay | Drawn | Color |
  | :- | :- | :- |
  | Colliders | each solid `Collider`'s box, or circle of 24 sides, at its entity's `Transform` | cyan |
  | Triggers | the same, of each trigger | magenta |
  | Tile grid | the collision grid's solid cells, filled | cyan, faint |
  | Nav grid | a dot two virtual pixels wide in the middle of each open cell of the grid paths cross: `Navigation`'s own, or else the collision grid (M9.5) | green |
  | Paths | each path of `Navigation` found: its legs, and a mark three pixels wide at each point after its first (M9.5) | yellow |
  | Spatial hash | each cell that holds colliders, as the last tick's rebuild left it (`hash_cells`), with its count | white, faint |
  | Tile chunks | each chunk of each tile layer drawn this frame, `TileLayer::CHUNK` tiles a side, cut at the layer's edge | orange |
  | Camera | a cross at the camera's position, 6 virtual pixels each way, and the view's rect: the zoom rounded to a whole number, at least 1, about the position and the shake | red |

  The nav grid and the paths are M9.5's (ADR-0069). The engine has no
  camera limits, so the camera's overlay is its view.
- **Drawing:** sprites of the debug UI's white texel in the world's
  sprites, layer 255 at `SortMode::Fixed(u32::MAX)`, in front of every
  sprite, as the inspector's outline: an outline is four sprites inside the
  rect, its corners first put on whole virtual pixels, as the renderer puts
  each sprite's, so that the edges meet and never overlap (a rect too thin
  for two edges is one sprite, a pixel each way at least); a fill's corners
  too; a line, one sprite turned about its start; lines a virtual pixel
  wide (one world unit over the zoom). A child's collider (`Parent`) is
  drawn where its root is drawn, with its offset, as its sprite would be.
  A box of no area is drawn. The hash's counts are world texts in the debug
  font, in the same layer. Render's `DebugDrawList` is not built. A steady
  frame of every overlay allocates nothing once the frame's buffers have
  grown.
- **Time:** the keys reach the debug tools as the window's events come,
  before the frame's clock, so they act in the frame they were pressed in,
  whether a debug field has the keyboard or not; the game's actions see
  them while no debug field has the keyboard, as they see a window's
  toggle (the capture holds every key back while one has). They change the time the clock is given (`DebugTools::
  game_time`, before `FixedClock::begin_frame` in `App::update`), so ticks,
  animated tiles and clips (`RenderFrame::time`) pause, step and scale
  together, and a step takes a replay's next tick:
  - **F5** pauses the game's time, and runs it again, dropping steps not
    taken; a repeat does nothing.
  - **F6** steps a tick: a frame's worth of a tick's length rounded up to
    a nanosecond, which runs one tick and keeps the frame's place between
    two (another tick after 50 million steps, as `App::update` says of
    such frames). It pauses first if the time runs; held, its repeats step
    on; the steps asked for in a frame run in its clock, at most 15 a
    frame (the clock takes 250 ms of a frame at most).
  - **F7** steps the scale up through 0.25, 0.5, 1, 2 and 4, and with
    Shift down, wrapping at either end; from a scale between two (the
    console's `timescale 0.3`), the next one past it. Paused, a step is a
    tick whatever the scale.
- **The label:** while the time is paused or not at 1, a panel at the
  screen's top-right, 8 units in, in the debug font: `paused`, `paused,
  x0.5` or `time x2`, pushed before the windows, which cover it where they
  reach the corner. Each pause and change of scale is logged too
  (`info!`), so the console shows it; a change of scale while paused says
  the time will run at it once F5 runs it again, as the console's
  `timescale` does. A step is logged at `debug!`, which the console shows
  once the log's level lets it through (`log level nadir debug`). The
  debug UI is made for the label if no window has made it.
- **Not built:** sound pausing with the game (music and voices play on),
  render's `DebugDrawList`, camera limits.

## Tools

| Tool | Toggle | Function |
| :- | :- | :- |
| Stats overlay | F1 | FPS, frame ms (CPU), tick ms, ticks this frame, draw calls, sprites, entities, allocations/frame, bytes allocated, audio voices, underruns |
| Inspector | F2 | Entity list (filter by name/component), selection highlight in world, component editors via `Reflect`, add/remove component, despawn, "save as prefab" (writes JSONC) |
| Console | ` (backtick) | Command line with history and Tab completion; output from the log ring buffer |
| Profiler | F3 | Frame-time graph (300 frames), click a frame for its scope tree with durations, `profile dump` writes Chrome trace JSON |
| Debug draw | F4 | Colliders, trigger volumes, spatial hash cells, nav grid, current paths, chunk bounds, camera bounds |
| Time control | F5–F7 | Pause/step one tick/time scale (0.25×–4×) |

## Console commands (initial set)

- `help`
- `schedule` (prints system order)
- `spawn <prefab> [x y]`
- `tp <x> <y>`
- `god`: left to games (M7.5, the user's choice): the engine has no health
  or damage to act on
- `timescale <f>`
- `level <name>`
- `reload` (forces re-read of all cooked files; as built, every loaded
  asset's file, `AssetServer::reload_all`)
- `profile dump [path]`
- `log level <target> <level>`
- `quit`

Games register their own commands with `console.register(name, help, fn)`;
as built (M7.5), `App::add_console_command(usage, help, command)`, over
nadir-debug's `Console::register(usage, help, command)`.

## Profiler

- `nadir_core::profile_scope!` records into thread-local buffers.
  `profile::take_frame` collects them at frame end.
- Ring buffer of the last 300 frames.
- **Export format:** Chrome Trace Event JSON, which opens in Perfetto UI or
  `chrome://tracing`.

  ```json
  {"traceEvents":[{"name":"render","ph":"X","ts":1234.5,"dur":812.0,"pid":1,"tid":1}]}
  ```

  `ts` and `dur` are microseconds. Write the file with our own JSON writer.
- **GPU timings (optional):** GL timer queries (`GL_TIME_ELAPSED`, core in
  3.3) around each pass, read back two frames later so we never stall.
  Metal: the command buffer's `GPUStartTime`/`GPUEndTime`. The timers are
  built (M7.11, ADR-0060): nadir-render's `Renderer::gpu_times`, each
  render's two passes read back two renders later. The profiler shows
  them beside the processor's time, and the trace has a GPU track (M7.12,
  ADR-0061, "As built (M7.12)" above).

## Tests

- **UI logic without rendering:** feed synthetic input, then assert widget
  results:
  - a button clicks on release only when the press started on it
  - slider clamping
  - text input editing
- **Inspector:** round-trip editing a component through `Reflect` changes
  the world value. As built (M7.4): nadir-debug's
  `editing_a_component_through_reflect_changes_the_world_value` edits a
  number, a float, an enum, a checkbox, a pair, a color, a nested field
  and a string through the window and reads the world's value back.
- **Trace export:** output parses with our JSON parser; event count and
  nesting are correct.
- **Golden:** a fixed debug window layout, so we notice visual regressions
  in the tools.
