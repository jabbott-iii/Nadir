---
name: phase-gates
description: Cluster roadmap tasks into phases that each end in a gate of checkable criteria, or evaluate a phase's gate with evidence gathered in this session and record the result.
argument-hint: "[plan | a phase ID such as M3; empty shows every gate's state]"
disable-model-invocation: true
---

# Cluster the roadmap into phase gates, or check a gate

Requested: `$ARGUMENTS`.

- `plan`: group the roadmap's tasks into phases, each ending in a gate.
- A phase ID, such as `M3`: evaluate that phase's gate.
- Empty: show every gate's state, the current phase, and what blocks it.

A phase is a milestone of the roadmap that `roadmap-and-docs.md` describes, and its gate is the
milestone's exit criteria, written as checks. Bold names are values from the "Project details"
of the adopted rules: **Docs folder**, **Task IDs**, **Environments**, **Definition of done**,
**Decision log**, **Status locations**, and **Gate evidence** from `roadmap-and-docs.md`, and
**Git access** from `commits.md`. If the repository hasn't adopted `roadmap-and-docs.md`, say so
and stop.

## The gate format

Write every gate in this shape, at the end of its phase, in place of free-form exit criteria:

```markdown
**Gate (M3):** not passed.
- [ ] <A criterion someone can check, about the phase as a whole.>
  - Check: `<command>` (expect <result>), or <what a person observes, where, and how>
  - Evidence: <the date, the commit, and the key output or number, once checked>
```

- A gate checks the phase as a whole. Don't repeat each task's `Accept` criteria; the tasks'
  ticks already cover them.
- Give every criterion a check: an exact command with its expected result, or, when no command
  can do it, what a person must observe, where, and on what hardware.
- Record evidence where the **Gate evidence** says. When that is the roadmap itself, use the
  `Evidence:` lines; otherwise write `Evidence: see <file>` and record it there.
- When a gate passes, its first line becomes `**Gate (M3):** passed 2026-11-22.` A gate's state
  is status, so keep it in agreement with the **Status locations**.

## Plan: cluster tasks into phases

### 1. Collect the work

1. Open the roadmap (`roadmap.md` in the **Docs folder**).
2. Collect the tasks to cluster: every unticked task, unless the user named a range, a milestone,
   or backlog items to bring in. Leave ticked tasks and passed gates where they are.
3. Read each task's `Depends`, `Decide first`, and `⏳ needs …` lines, the specs its `Do` lines
   point to, and the records it relies on in the **Decision log**.

### 2. Find the structure

- **Dependencies:** order the tasks so that every task comes after everything in its `Depends`
  line. Add a dependency a task needs but doesn't list, and say so.
- **Deliverables:** group the tasks that together produce one result a user can see or use: a
  window that opens, a request that's answered, a build that ships. Prefer a thin slice through
  every layer to one layer at a time, so each phase shows the parts working together.
- **Risk first:** put the experiments, spikes, and riskiest assumptions in the earliest phases,
  so a wrong assumption is found before work builds on it.
- **Environments:** within a phase, order platform work by the **Environments**. Put work that
  needs an environment the team rarely has where waiting for it blocks the least.
- **Size:** keep each phase small enough to check in one gate, about 3 to 10 tasks. Split a
  bigger cluster into two phases with a gate between them.

### 3. Write each phase's gate

Give each phase a goal and a gate whose criteria cover:

- **Integration:** the phase's parts working together, checked end to end, such as a scenario
  run or a demo with its steps.
- **Budgets:** each budget the phase affects, measured on the hardware the overview names.
- **Quality:** the full check from the **Definition of done**, passing.
- **Open work:** no `⏳ needs …` line left in the phase, unless the user decides to carry one
  into a later gate, and that gate names it.
- **Decisions:** no decision record the phase relies on still `Proposed`.
- **Documents:** the specs, the feature list, and the **Status locations** describing what the
  phase built.
- **Risks:** each experiment the phase ran answered, with its result recorded.

### 4. Propose, then write

1. Show the user the proposal before you change the roadmap: a table of each phase with its
   goal, its tasks, the number of gate criteria, and why it comes where it does. List every task
   that would move, and every dependency you added.
2. Moving tasks between milestones is the user's decision, as `roadmap-and-docs.md` says. Write
   nothing until the user approves; then follow their changes.
3. Write the approved phases into the roadmap:
   - Keep every task ID. A task that moves keeps its ID and gets a dated note:
     `Note (2026-11-22): moved from M5 to M3, the user's choice.`
   - Give each new phase the next free milestone number in the **Task IDs** format, a goal, and
     its gate.
   - Replace free-form exit criteria with the gate the user approved.
4. Update the **Status locations** if they name the phases.

## Evaluate a gate

### 1. Read the gate and its phase

1. Find the phase's gate, and its earlier evidence where the **Gate evidence** says.
2. List the phase's tasks that aren't ticked, and every `⏳ needs …` line. A gate can't pass
   while one remains, unless the gate names it as carried.
3. Confirm that every earlier phase has passed its gate. If one hasn't, say which, and evaluate
   this gate only if the user asks.

### 2. Collect the evidence

- Gather evidence for each criterion in this session: run its command, read the log, take the
  measurement, or ask the user for an observation you can't make yourself.
- Evidence from an earlier session counts only if it was recorded, with its commit, and nothing
  in its area has changed since. Check with `git diff --stat <commit>..HEAD` over that area.
- Run commands only in ways that change nothing the project ships: checks, tests, and
  measurements, never a deployment or a release.
- Record measured numbers with their unit, the hardware, and the method, and both sides of any
  comparison the criterion makes.

### 3. Judge the gate

Show a table of each criterion, its evidence (the command and its key output, or the number), and
its result: **pass**, **fail**, or **not evaluated**. Any fail or not evaluated means the gate is
**not passed**.

### 4. Record the result

- Record each criterion's evidence where the **Gate evidence** says, with the date and the
  commit.
- **If every criterion passes,** ask the user to confirm the gate, because a human checks a
  milestone's exit criteria. On their yes, tick the criteria, write the pass date on the gate's
  line, update the **Status locations**, and name the next phase as current.
- **If the gate doesn't pass,** leave it unticked, and list what blocks it: each failed or
  unevaluated criterion, and each open task. Add a task for a failure only if the user agrees.

## Show every gate's state

Without an argument, list each phase with its gate's state (passed with its date, not passed, or
no gate yet), the current phase (the first whose gate hasn't passed), and what blocks it: open
tasks, `⏳ needs …` lines, and criteria without evidence. Change nothing.

## Commit only as the project allows

Follow the **Git access** level in `commits.md`, and leave the changes uncommitted unless the user
asked for a commit and that level allows it. Either way, end the report with a copy-ready commit
message, as `commits.md` describes.

## Report

End with a summary that contains:

- **Mode:** plan, evaluate, or state.
- **Phases:** for a plan, each phase with its goal, its tasks, and its gate; for an evaluation,
  the gate's table.
- **Moved tasks and added dependencies,** for a plan.
- **Result:** for an evaluation, passed (with the user's confirmation) or not passed, and what
  blocks it.
- **Not evaluated:** each criterion without evidence, and why.
- **Next:** the current phase, and the first task to work on in it.
- **Commit message:** in a `text` block, as `commits.md` describes, with no attribution lines.
