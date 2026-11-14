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
timer queries are M7.11's.

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
- `god`
- `timescale <f>`
- `level <name>`
- `reload` (forces re-read of all cooked files)
- `profile dump [path]`
- `log level <target> <level>`
- `quit`

Games register their own commands with `console.register(name, help, fn)`.

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
  Metal: the command buffer's `GPUStartTime`/`GPUEndTime`. Not built yet:
  roadmap M7.11 (ADR-0059).

## Tests

- **UI logic without rendering:** feed synthetic input, then assert widget
  results:
  - a button clicks on release only when the press started on it
  - slider clamping
  - text input editing
- **Inspector:** round-trip editing a component through `Reflect` changes
  the world value.
- **Trace export:** output parses with our JSON parser; event count and
  nesting are correct.
- **Golden:** a fixed debug window layout, so we notice visual regressions
  in the tools.
