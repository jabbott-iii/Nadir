# Nadir engine — design and build plan

This folder is the design for a cross-platform (desktop) 2D top-down game
engine written in Rust, with almost no third-party dependencies. It is laid out
so Claude Code (or another coding agent) can start building it right away.

## How to use it

1. Create an empty Git repository and copy everything in this folder into its
   root, including the hidden `.claude` directory.
2. Start Claude Code in the repository root. It loads `CLAUDE.md`, which
   imports `AGENTS.md`.
3. Run `/next-task`. It picks up `M0.1` and works through the roadmap one task
   at a time, leaving each change uncommitted for you to review.

## What is in here

| Path | Contents |
| :- | :- |
| `AGENTS.md` | Project rules for any coding agent: scope, constraints, workspace map, commands, conventions, definition of done |
| `CLAUDE.md` | Imports `AGENTS.md`, plus Claude Code skills, rules and permissions |
| `docs/00-overview.md` | Goals, non-goals, budgets, assumptions, scope estimate |
| `docs/01-architecture.md` | Crates, dependency matrix, main loop, data flow, threading |
| `docs/02-dependency-policy.md` | The build-it-ourselves rule, the exception process, the build-vs-buy register |
| `docs/03-platforms.md` | Windows, macOS and Linux specifics |
| `docs/04-quality.md` | Testing strategy, own test harnesses, CI, `xtask` checks |
| `docs/05-release.md` | Build profiles, packaging, signing, notarization, crash reports |
| `docs` | One spec per subsystem: responsibilities, API sketch, invariants, tests |
| `docs/roadmap.md` | Milestones M0–M11 plus backlog, with checkbox tasks and acceptance criteria |
| `docs/decisions.md` | Architecture decision records (ADRs) |
| `.claude/settings.json` | Permission guardrails for Claude Code |
| `.claude` | Path-scoped rules for unsafe/FFI, dependencies, parsers, shaders |
| `.claude` | `/next-task`, `/evaluate-dependency`, `/add-ffi-binding`, `/record-decision` |

## The two constraints that shape everything

1. **Desktop only:** Windows, macOS, Linux. Dropping web and mobile lets the
   engine own a simple polling main loop, use plain threads instead of async,
   and avoid browser and mobile lifecycle constraints. See ADR-0001.
2. **Build it unless it's unreasonable:** zero third-party runtime crates. OS
   bindings, the graphics backends, codecs, the ECS, the mixer, the debug UI
   and the test harnesses are all ours. See `docs/02-dependency-policy.md` and
   ADR-0002.
