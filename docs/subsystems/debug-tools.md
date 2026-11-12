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
                         // wants_mouse, wants_keyboard, places, set_place, places_revision
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
  Metal: the command buffer's `GPUStartTime`/`GPUEndTime`.

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
