# Diagrams

Pictures of how Nadir is built, drawn from the code at commit `0cba65f`
(version 0.2). Each file is Markdown with Mermaid blocks, which GitHub
draws. What the roadmap plans but the code does not have yet is left out
or marked as planned.

| File | Shows |
| :- | :- |
| `c4-architecture.md` | C4 views: the system and what it deals with, its programs and the files between them, and the crates inside the game and the cook, by layer, with each crate's dependencies |
| `game-loop-sequence.md` | Sequence diagrams of the game loop: setup, opening the window, one pass of the main loop, the 60 Hz ticks, the frame stages, drawing and presenting, the asset workers, shutdown, and frames with no window |
| `class-diagram.md` | Class diagrams of the main types and how they relate: core's handles and math, the ECS, assets, scene, render, gfx, collision and the facade |

The design is in `docs/`: `docs/01-architecture.md` for the crates and the
main loop, and `docs/subsystems/` for each crate. When the code and a
diagram disagree, the code is right: a change that alters a crate's
dependencies, the main loop or a type drawn here should update the diagram
too.
