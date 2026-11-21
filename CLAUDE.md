@AGENTS.md

## Claude Code specifics

The project rules are in `AGENTS.md` (imported above), shared with any other
coding agent. This section covers only what is specific to Claude Code.

**Project skills** (in `.claude/skills`):

| Skill | Use it to… |
| :- | :- |
| `/next-task [task-id]` | Pick the next unblocked roadmap task, or the one named, and carry it through the definition of done |
| `/evaluate-dependency <crate> <need>` | Run the build-vs-buy evaluation before anyone proposes a third-party crate |
| `/add-ffi-binding <os> <api>` | Add OS bindings to `nadir-sys` and wrap them safely in `nadir-platform` or `nadir-gfx` |
| `/record-decision <title>` | Append an ADR to `docs/decisions.md` |

**Path-scoped rules** in `.claude/rules` load automatically when you open
matching files:

| Rule file | Applies to |
| :- | :- |
| `unsafe-and-ffi.md` | `sys`, `platform`, `gfx` |
| `dependencies.md` | Cargo manifests and `xtask` |
| `parsers.md` | `codec`, `assets`, `scene` (saves, prefabs), the cook tool, fuzz corpora |
| `shaders.md` | GLSL/MSL shader sources |
| `documentation.md` | Markdown files, `docs/`, `diagrams/`, `site/`, and Rust doc comments: plain wording, American English, and a two-reader check |

**Permissions:**

- `.claude/settings.json` pre-approves routine `cargo` commands.
- It denies Git operations that publish or finalize changes, plus
  `cargo publish`, `cargo login` and `make release` (which tags and pushes).
- It asks before `cargo add`, `cargo remove`, `cargo update` and
  `cargo install`.
- It sets `attribution.commit` to `""`, which turns off Claude Code's own
  commit attribution (see "Commit messages" below).
- These are guardrails, not a security boundary. The rules in `AGENTS.md` still
  apply when a command slips past a pattern.

**Working style:**

- Use the task list for any roadmap task with more than two steps.
- Make the last item "verify", and run the task's verification commands for it.
- Most tasks touch one crate. If a change spreads across more than three
  crates, stop and say so: the task is probably mis-scoped.
- When an acceptance criterion needs a specific OS this session can't run
  (Claude Code sessions are usually Linux), implement it and say plainly
  which checks still need that OS.

**The maintainer's instructions:** the maintainer's account instructions for
Claude apply here too. `AGENTS.md` already covers most of them: the smallest
complete change, honest verification, no secrets, read-only Git and the final
summary. The rest:

- Work as a pair programmer. Explain significant decisions briefly, name the
  risks and assumptions, and leave decisions to the user.
- In a report, keep facts, assumptions, recommendations and unknowns apart.
- Never weaken authentication, validation, encryption, logging, tests, type
  checks, lints or other security controls to finish a task.
- When instructions conflict, don't silently take a path that weakens
  security, breaks compatibility or can't be undone. State the conflict and
  propose the safest option that complies.

**Commit messages:**

- Agents still never commit (`AGENTS.md`, rule 7): the user does.
- When a piece of work is done and verified (a roadmap task, a fix, a docs
  change, or a checkpoint in a long task), pause and offer a commit message
  for it, in a `text` block at the end of the message. Don't offer one for
  work still in progress.
- The subject follows `CONTRIBUTING.md` → "Commits and pull requests", in the
  shape recent commits have (`git log`), as in
  `M8.1 core, sys and platform: Linux gamepads from evdev, with inotify hotplug`.
- The body says what changed and why, as recent commits do: the decision and
  the task's state, then each crate's changes, then the docs. Wrap it at 72
  columns.
- **No attribution lines:** no `Co-Authored-By:`, `Signed-off-by:`,
  `Claude-Session:` or "Generated with Claude Code". This overrides Claude
  Code's default commit attribution.
