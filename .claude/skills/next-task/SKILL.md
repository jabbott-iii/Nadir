---
name: next-task
description: Pick the next unblocked task from the roadmap (or the task ID given) and carry it through the project's definition of done, leaving the work ready for the user to review.
argument-hint: "[task-id, e.g. M1.2]"
disable-model-invocation: true
---

# Work the next roadmap task

Requested task: `$ARGUMENTS` (empty means "pick the next unblocked task").

This skill works on a roadmap kept as `roadmap-and-docs.md` describes. Bold names such as
**Definition of done** are values from that rule's "Project details", and **Git access** comes
from `commits.md`. If the repository hasn't adopted those rules, say so and stop.

## 1. Choose the task

1. Open the roadmap (`roadmap.md` in the **Docs folder**).
2. If a task ID was given, use it. If any task in its `Depends` line isn't ticked, stop and tell
   the user which ones block it.
3. Otherwise, take the **first** unticked task whose `Depends` tasks are all ticked.
   - Skip a task whose remaining work is marked `⏳ needs <environment>` when this session can't
     provide that environment (see the **Environments**).
   - Tell the user which tasks you skipped, and why.
4. Check the task's gates before you write any code that relies on them:
   - **`Decide first`:** ask each question the line names, unless the **Decision log** already
     records the answer. Lay out the options with their costs, and recommend one.
   - **A `Proposed` decision record** the task depends on: show its decision in two or three
     lines, and ask the user to accept, change, or reject it.
   - **An unfinished experiment or open decision** the task waits on: the task stays blocked
     until it is done. Say what it waits on.

   Record each answer as `roadmap-and-docs.md` describes, before writing code: accept a record by
   changing only its `Status` line, or write a new record for a changed decision. If the user
   isn't there to answer, stop after the reading in step 2 and say what is waiting on them.
5. Tell the user in one or two sentences which task you're starting and how you plan to approach
   it. Then make a task list whose last item is "Verify and report".

## 2. Load the context, and only the context you need

- Read the documents that the **Agent guide** lists for the areas the task touches, starting with
  the spec sections its `Do` lines point to.
- Read the existing code in those areas before you write new code, and follow its patterns and
  naming. The language and platform rules for the files you touch load on their own; follow them.
- If the spec and the code disagree, or the spec is ambiguous in a way that would change a public
  API, a stored or file format, a protocol, or which parts may depend on which: stop, describe
  the conflict, propose the safest option, and ask.

## 3. Make the smallest complete change

- Do exactly what the task's `Do` lines and `Accept` criteria require, and nothing else: no
  refactors, renames, or reformatting of code the task doesn't touch.
- **A new third-party dependency?** Stop. Follow the project's dependency policy, and don't add
  it until the user approves.
- **A kind of change the project has a skill for** (a dependency evaluation, a decision record, a
  format change)? Read that skill's `SKILL.md` and follow its steps. A skill only the user can
  start can still be read and followed.
- **A design decision changed?** Record it as `roadmap-and-docs.md` describes, as `Proposed`
  unless the user decided it in this conversation, and update every document that states it.
- Write the tests the `Accept` criteria call for, beside the code. For a bug, write the failing
  test first.
- If the change spreads well beyond the areas the task names (more than about three modules or
  packages), pause and tell the user the task may be scoped wrong.

## 4. Verify honestly

- Run every command in the task's `Verify` line, then the command that runs every check, as the
  **Definition of done** names it.
- Fix a failure by fixing the code. **Never** weaken a lint, delete or skip a test, loosen an
  expectation or a tolerance, raise a limit, or accept new golden or snapshot output to make a
  check pass, unless the user explicitly agrees.
- For a criterion that needs another environment, real hardware, or a person to watch:
  1. Do everything you can here.
  2. Leave the box unticked.
  3. Add an indented `⏳ needs <environment>: <the exact check that remains>` line under the
     task.

## 5. Update the roadmap and the documents

- Tick the box **only** if every `Accept` criterion is met and every `Verify` command ran and
  passed in this session.
- Add brief, indented `Note:` lines under the task where they help a later reader: pinned
  versions, measured numbers, seeds, conventions chosen, and anything left for later. Put a note
  meant for a later task under that task, as `Note (from M2.3): …`.
- Update the spec wherever the change altered behavior or design.
- Once the box is ticked, bring the **Feature list** and the **Status locations** up to date as
  `roadmap-and-docs.md` describes. Then run the **Feature list**'s check again, because the full
  check in step 4 ran before the tick.
- If you edited any of the **Checked documents**, run their checks.

## 6. Commit only as the project allows

Follow the **Git access** level in `commits.md`. Unless the user asked for a commit in this
conversation and that level allows it, leave every change uncommitted, and never amend, rebase,
tag, or push. Either way, end the report with a copy-ready commit message, as `commits.md`
describes.

## 7. Report

End with a summary that contains:

- **Task:** its ID and title, and whether its box is now ticked.
- **Decisions:** each question you asked, the user's answer, and where you recorded it.
- **Changes:** what changed, and the files affected.
- **Validation:** each command you ran, with its real result.
- **Not run:** each check that didn't run, and why, including every `⏳` line.
- **Assumptions** you made.
- **Open issues and risks** that remain.
- **Next:** the next unblocked task's ID.
- **Commit message:** in a `text` block, as `commits.md` describes, with no attribution lines.
