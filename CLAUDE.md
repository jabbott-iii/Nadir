@AGENTS.md

## Claude Code specifics

The project rules are in `AGENTS.md` (imported above), shared with any other
coding agent. This section covers only what is specific to Claude Code.

**Project skills** (in `.claude/skills`):

| Skill | Use it to… |
| :- | :- |
| `/next-task [task-id]` | Pick the next unblocked roadmap task, or the one named, and carry it through the definition of done |
| `/evaluate-dependency <crate> <need>` | Run the build-vs-buy evaluation before anyone proposes a third-party crate |
| `/add-ffi-binding <os> <api>` | Add OS bindings to `nadir-sys` and wrap them safely in `nadir-platform` |
| `/record-decision <title>` | Append an ADR to `docs/decisions.md` |

**Path-scoped rules** in `.claude/rules` load automatically when you open
matching files:

| Rule file | Applies to |
| :- | :- |
| `unsafe-and-ffi.md` | `sys`, `platform`, `gfx` |
| `dependencies.md` | Cargo manifests and `xtask` |
| `parsers.md` | `codec`, `assets`, `scene` (saves, prefabs), the cook tool, fuzz corpora |
| `shaders.md` | GLSL/MSL shader sources |

**Permissions:**

- `.claude/settings.json` pre-approves routine `cargo` commands.
- It denies Git operations that publish or finalize changes, plus `cargo publish`.
- It asks before `cargo add`, `cargo update` and `cargo install`.
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
