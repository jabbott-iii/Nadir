---
name: next-task
description: Pick the next unblocked task from docs/roadmap.md (or the task ID given) and carry it through the project's definition of done, leaving changes uncommitted for review.
argument-hint: "[task-id, e.g. M1.2]"
disable-model-invocation: true
---

# Work the next roadmap task

Requested task: `$ARGUMENTS` (empty means "pick the next unblocked task").

## 1. Choose the task

1. Open `docs/roadmap.md`.
2. If a task ID was given, use it. If its `Depends` tasks are not all
   checked, stop and tell the user which ones block it.
3. Otherwise, take the **first** unchecked task whose `Depends` are all
   checked.
   - Skip a task whose remaining work is marked `⏳ needs <OS>` when this
     session cannot run that OS.
   - Mention any task you skipped and why.
4. Tell the user in one or two sentences which task you're starting and how
   you plan to approach it. Then create a task list. Its last item is
   "Verify and report".

## 2. Load context, and only the context you need

- `AGENTS.md` is already loaded. Re-read its "Non-negotiable rules" if
  unsure.
- Read the docs that `AGENTS.md` → "Read before working" lists for the
  crates the task touches, especially the relevant subsystem spec section.
- Read the existing code in those crates before writing new code. Follow
  established patterns and naming.
- If the spec and the existing code disagree, or the spec is ambiguous in a
  way that changes a public API, an on-disk format, or the dependency
  matrix: stop, describe the conflict, propose the safest option, and ask.

## 3. Implement the smallest complete change

- Do exactly what the task's `Do` lines and acceptance criteria require.
  Nothing else: no drive-by refactors, renames or formatting of untouched
  code.
- **New third-party crate needed?** Stop and run `/evaluate-dependency`.
  Don't add it.
- **OS bindings needed?** Follow `/add-ffi-binding`.
- **A design decision changed?** Run `/record-decision` and update the
  affected doc.
- Write the tests the acceptance criteria call for, beside the code.
- If the change spreads beyond three crates, pause and tell the user the
  task may be mis-scoped.

## 4. Verify honestly

- Run every command in the task's `Verify` line.
- Then run `cargo xtask ci`. If `xtask` doesn't exist yet (before M0.6),
  run `cargo fmt --all -- --check`,
  `cargo clippy --workspace --all-targets -- -D warnings` and
  `cargo test --workspace` instead.
- Fix failures by fixing the code. **Never** weaken a lint, delete or
  `#[ignore]` a test, raise a tolerance, or re-bless golden images to make
  a check pass, unless the user explicitly agrees.
- Criteria that need another OS, real hardware, or manual observation:
  1. Do everything possible here.
  2. Leave the box unchecked.
  3. Add an indented `⏳ needs <OS/hardware>: <exact remaining check>` line
     under the task in the roadmap.

## 5. Update the roadmap and docs

- Check the box **only** if every acceptance criterion is met and every
  verification command ran and passed in this session.
- Add brief indented notes under the task where useful: the pinned
  toolchain version, measured numbers, recorded findings.
- Update docs only where behavior or design actually changed.

## 6. Do not commit

Leave all changes uncommitted. Do not commit, amend, rebase, tag or push.

## 7. Report

End with a summary containing:

- **Task:** ID and title, and whether its box is now checked.
- **Changes:** what changed, and the files affected.
- **Validation:** each command run, with its real result.
- **Not run:** checks that weren't run, and why. Include `⏳` items.
- **Assumptions** you made.
- **Unresolved issues and residual risks.**
- **Next:** the next unblocked task ID.
