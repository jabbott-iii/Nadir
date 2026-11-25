@AGENTS.md

## Claude Code specifics

The project rules are in `AGENTS.md` (imported above), shared with any other
coding agent. This section covers only what is specific to Claude Code.

**Project skills** (in `.claude/skills`). Nadir's own:

| Skill | Use it to… |
| :- | :- |
| `/evaluate-dependency <crate> <need>` | Run the build-vs-buy evaluation before anyone proposes a third-party crate |
| `/add-ffi-binding <os> <api>` | Add OS bindings to `nadir-sys` and wrap them safely in `nadir-platform` or `nadir-gfx` |
| `/record-decision <title>` | Append an ADR to `docs/decisions.md` |

The rest come from [Doctrina](https://github.com/jabbott-iii/Doctrina)
(commit `3afa240`), unchanged, and read the "Project details" of the rules
below. Only you can start them:

| Skill | Use it to… |
| :- | :- |
| `/next-task [task-id]` | Pick the next unblocked roadmap task, or the one named, and carry it through the definition of done |
| `/phase-gates [plan \| M<n>]` | Group roadmap tasks into gated phases, or check a phase's gate |
| `/fix-doc-drift [scope]` | Find and correct docs that no longer match the code |
| `/draw-diagrams [file]` | Draw or rebuild the Mermaid diagrams in `diagrams/` |
| `/sweep-code [scope] [bugs \| dead code]` | Find bugs and dead code, prove each, and fix the approved ones |
| `/simplify-code [scope]` | Simplify long or convoluted code without losing behavior |
| `/security-sweep [scope]` | Find vulnerabilities, prove each locally, and patch the approved ones |
| `/set-up-github [pieces]` | Write GitHub's files (CodeQL, Dependabot, `SECURITY.md`, templates); Nadir has most already |
| `/build-language-packs [languages] [path]` | Translate user-facing text into nine languages; needs a localization system Nadir lacks |
| `/build-course [description]` | Build a course; needs `teaching-materials.md`, which Nadir doesn't have |

**Rules** in `.claude/rules`. These four, from Doctrina, load in every
session:

| Rule file | Covers |
| :- | :- |
| `commits.md` | Commit messages (Conventional Commits), read-only Git, pull requests |
| `dependencies.md` | Anything the project didn't write: none from outside the workspace but Ferrolua's crates (ADR-0079) |
| `testing.md` | Strict tests, coverage, performance, security, CI |
| `writing.md` | Everything written for people: plain wording, American English, the two-reader check |

These load automatically when you open matching files:

| Rule file | Applies to |
| :- | :- |
| `rust.md` (Doctrina) | Rust sources, Cargo manifests, the toolchain files |
| `roadmap-and-docs.md` (Doctrina) | `docs/`, `AGENTS.md`, `CONTRIBUTING.md`, `README.md`, `diagrams/` |
| `unsafe-and-ffi.md` | `sys`, `platform`, `gfx` |
| `parsers.md` | `codec`, `assets`, `scene` (saves, prefabs), the cook tool, fuzz corpora |
| `shaders.md` | GLSL/MSL shader sources |

Each Doctrina rule's "Project details" section holds Nadir's values, which
the rest of the rule names in bold. Keep them true when the project
changes.

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

**Commit messages:** follow `.claude/rules/commits.md`.

- Agents still never commit (`AGENTS.md`, rule 7): the user does.
- When a piece of work is done and verified (a roadmap task, a fix, a docs
  change, or a checkpoint in a long task), offer a commit message for it, in
  a `text` block at the end of the message. Don't offer one for work still
  in progress.
- Since 2026-11-24, the user's choice, messages are Conventional Commits:
  `<type>(<scope>): <summary>`, with a roadmap task's ID in parentheses at
  the end, as in `feat(scene): restore a save over its level (M9.8)`.
  Commits before that date have the old shape,
  `M8.1 core, sys and platform: Linux gamepads from evdev`.
- **No attribution lines:** no `Co-Authored-By:`, `Signed-off-by:`,
  `Claude-Session:` or "Generated with Claude Code". This overrides Claude
  Code's default commit attribution.
