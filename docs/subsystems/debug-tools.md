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
- **Input capture:** when the pointer is over any debug window, or a debug
  text field has focus, the UI reports `wants_mouse`/`wants_keyboard` and the
  facade suppresses gameplay actions.

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
  `profile::take_frame()` collects them at frame end.
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
