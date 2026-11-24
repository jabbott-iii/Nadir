---
paths:
  - "docs/**"
  - "AGENTS.md"
  - "CONTRIBUTING.md"
  - "README.md"
  - "diagrams/**"
  - "diagram/**"
---

# Roadmap and documentation standards

These rules cover a project's planning and design documents: the overview and the other core
documents, the roadmap, the decision log, the specs for each part of the system, the feature
list, and the places that report status. Together they let a new contributor, human or agent,
find out what the project is, what is built, what comes next, and why each decision was made.
How to write the text itself is in `writing.md` in this folder.

Apply them to documents you write or edit. Don't restructure an existing document only to make
it conform; bring a section into shape when you are already changing it.

## Project details

A repository fills in this section when it adopts the rule, and the rest of the rule refers to
each value by its bold name. If a value still reads `[FILL IN: …]`, the rule isn't set up here
yet. Work out that value from the repository itself, and tell the user which values are missing
so they can fill them in.

- **Docs folder:** `docs/`
- **Core documents:** `00-overview.md` (the purpose, goals, non-goals, performance budgets,
  assumptions, and scope), `01-architecture.md` (the layers and the dependency matrix, the main
  loop, threading, and feature flags), `02-dependency-policy.md`, `03-platforms.md`,
  `04-quality.md` (the tests, the checks, CI, and the demo page), and `05-release.md` (packaging,
  signing, crash reports, and engine releases)
- **Specs:** `docs/subsystems/<name>.md`, one for each crate or pair of crates
  (`sys-and-platform.md`, `assets-and-codecs.md`, `debug-tools.md`); "Read before working" in
  `AGENTS.md` says which spec covers which crate.
- **Task IDs:** `M<milestone>.<task>`, such as `M9.8`, with the areas their titles use: crate
  names (`core`, `sys`, `platform`, `gfx`, `render`, `codec`, `ecs`, `input`, `audio`, `assets`,
  `collision`, `scene`, `debug`, and the planned `script` and `studio`), `facade` for
  `crates/engine`, `cook`, `xtask`, `sandbox`, `content`, and `dev tools`, with the OS in
  parentheses for platform work, as in `M1.5 — sys + platform (Windows)`.
- **Environments:** Linux, then Windows, then macOS, within every milestone; then real hardware
  (gamepads, audio devices) and a person, for the checks by hand. Claude Code sessions here
  usually run Linux (x86_64) only.
- **Definition of done:** "Definition of done for a roadmap task" in `AGENTS.md`; `cargo xtask ci`
  runs every check.
- **Decision log:** `docs/decisions.md`, with `ADR-NNNN`; `/record-decision` appends a record.
- **Feature list:** `docs/features.jsonc`, kept in step with the roadmap by
  `cargo xtask features`; the demo page lists it (ADR-0029).
- **Status locations:** the roadmap's ticks and `⏳ needs` lines; the "Status" paragraph of
  `AGENTS.md`; the "Status" section of `README.md` and of each crate's `README.md` (such as "Not
  built yet" in `crates/engine/README.md`); and `docs/features.jsonc`, which the demo page shows.
- **Checked documents:** the dependency matrix in `docs/01-architecture.md`
  (`cargo xtask layers`); the roadmap's ticks against `docs/features.jsonc`
  (`cargo xtask features`); `xtask/deps-allowlist.txt`, the approved packages from the register
  in `docs/02-dependency-policy.md` (`cargo xtask deps`); the workflows in `.github/workflows/`
  (`cargo xtask workflows`); and each README's Rust blocks, which `cargo test --doc` compiles.
- **Agent guide:** "Read before working" and the workspace map in `AGENTS.md`, and the rule and
  skill tables in `CLAUDE.md`.
- **Update map:** "Documentation" in `CONTRIBUTING.md`.
- **Diagrams:** Mermaid in `diagrams/` (`c4-architecture.md`, `class-diagram.md`,
  `game-loop-sequence.md`), drawn from the code at the commit `diagrams/README.md` names; what the
  roadmap plans but the code lacks is left out or marked as planned.
- **Gate evidence:** none: each milestone ends in free-form "Exit criteria", which a person
  checks.

## Decisions that are the user's

The documents record the user's decisions, so some changes need the user's agreement first.
Stop and ask before you:

- accept a decision record (draft it as `Proposed` instead);
- add, remove, reorder, or rescope a milestone, or move a task to another milestone;
- change a goal, a non-goal, a budget, or an assumption in the overview;
- check a milestone's exit criteria or pass its gate, which a human confirms;
- or tick a task whose remaining check needs an environment you couldn't run.

When the user decides, record what they chose and the date, in the decision record's context or
in a dated note on the task.

## The document set

Keep the documents in the **Docs folder**, each with one job. Point to another document instead
of repeating what it says, so each fact has one place to update.

| Document | Its job |
| --- | --- |
| The **Core documents** | What the project is and the rules that hold everywhere |
| The roadmap | What is built, what comes next, and how each step is checked |
| The **Decision log** | Why each significant choice was made, and what would change it |
| The **Specs** | How each part works, what it guarantees, and how it is tested |
| The **Feature list** | What users get, in their terms, tied to roadmap tasks |

### Core documents

Number the core documents so they read in order. Keep the ones this project needs, from this
set:

- `00-overview.md`: the purpose, goals, non-goals, budgets, assumptions, scope estimate, and top
  risks.
- `01-architecture.md`: the principles, the layers and which may depend on which, the source
  layout, the main control flow, the threading model, and configuration.
- `02-dependency-policy.md`: the rule for third-party code, what counts as a dependency, the test
  an exception must pass, the process for one, and the register of build-or-buy decisions.
- `03-platforms.md`: the supported targets, the order they are developed in, what each needs,
  and the invariants tested on every one.
- `04-quality.md`: the test layers, the project's own test tools, the automated checks,
  continuous integration (CI), and the review checklists.
- `05-release.md`: build profiles, versioning, packaging, the release pipelines, crash
  reporting, and the release checklist.

Write the overview first, in this shape:

```markdown
# 00 — Overview

## Purpose
<What the project is, for whom, and the one rule that shapes the rest.>

## Goals
1. <A numbered, checkable goal.>

## Non-goals
- <What the project won't do, and the decision record that says so.>

## Budgets
| Budget | Target |
| :- | :- |
| <What is measured> | <The number, with its unit and the hardware it is measured on> |

## Assumptions
Revisit these if any turns out to be wrong.
- <An assumption the plan depends on.>

## Scope estimate
| Area | Size | Why |
| :- | :- | :- |
| <A part of the system> | <S, M, L, or XL, defined above the table> | <What makes it that size> |

## Top risks and mitigations
| Risk | Mitigation |
| :- | :- |
| <What could go wrong> | <What the plan does about it> |
```

- A non-goal names the decision record that excluded it, and anything that would bring it back
  needs a new record.
- Every budget is a number with a unit, measured on named hardware. If the hardware isn't chosen
  yet, say so and name the task that must record it.

## The roadmap

### How the file is organized

Keep the roadmap in `roadmap.md` in the **Docs folder**. Start it with a short "How to use this
file" list, then the milestones in order, then the backlog:

- Work from the top down. A task is unblocked when every task in its `Depends` line is ticked.
- Tick a task only after its acceptance criteria are met and its `Verify` commands ran and
  passed, as the **Definition of done** says.
- A milestone's exit criteria, or its gate, are checked by a human.
- Within each milestone, work moves through the **Environments** in their order.

### Milestones

Each milestone delivers something a user can see or use, and its title says what:

```markdown
## M3 — <What works at the end of the milestone>

**Goal:** <One or two sentences on what the milestone delivers.>

<The milestone's tasks.>

**Exit criteria (M3):** <What a human checks before the milestone closes.>

---
```

Where the **Gate evidence** isn't "none", end each milestone with a phase gate instead of free-form
exit criteria: checkable criteria about the milestone as a whole, each with its check and its
evidence. The `/phase-gates` skill clusters tasks into gated phases and evaluates a gate:

```markdown
**Gate (M3):** not passed.
- [ ] <A criterion about the milestone as a whole.>
  - Check: `<command>` (expect <result>), or <what a person observes, where, and how>
  - Evidence: <the date, the commit, and the key output or number, once checked>
```

### Tasks

Write every task in the same shape, so people and tools can read it:

```markdown
- [ ] **M3.4 — render: sprite batching**
  - Note: <A finding or a choice made while doing the task.>
  - ⏳ needs <environment>: <the exact check that remains>
  - Depends: M3.1, M3.3
  - Decide first: <a question the user must answer before work starts>
  - Do:
    - <A concrete step, naming the files, types, and spec sections it follows.>
  - Accept:
    - <An observable result that shows the step worked.>
  - Verify: `<command>`; `<command>` (expect <result>)
```

- Name the task by its area and what it builds: `M3.4 — render: sprite batching`, with an area
  from the **Task IDs**.
- `Note:` and `⏳ needs …` lines go first, under the title, so a reader sees the task's state
  before its plan. Omit `Decide first` when there is no open question.
- `Do` says what to build and points to the spec section it follows, rather than repeating the
  spec. `Accept` lists results someone can observe. `Verify` lists the exact commands, with the
  result each must give.
- When a check can't be automated, say in `Verify` what to observe instead, and say in the
  report which way it was checked.
- When part of a task needs an environment the session doesn't have, do everything else, leave
  the box unticked, and add `⏳ needs <environment>: <the exact check that remains>`. Remove the
  line once someone runs that check, and tick the box if nothing else remains.

### Changing the roadmap

- Task IDs never change and are never reused. A new task takes the next free number in its
  milestone and is listed where it is to be done, even if that breaks numerical order.
- When a task is split, the part that finishes what it promised keeps its ID, and the rest
  becomes a new task.
- Never rewrite a finished task's text. Add a `Note:` with the date instead, saying what changed
  and why.
- Record the facts a later task will need in a task's notes: pinned versions, measured numbers,
  conventions chosen, and anything left for another task, by its ID.
- When the user moves work between tasks or milestones, say so in a dated note on both, with
  "the user's choice".

### The backlog

End the roadmap with work that isn't planned yet. Every item needs a trigger: an observable
event that would put it on the roadmap.

```markdown
## Backlog (each needs a trigger or a decision)

| Item | Trigger / reason |
| :- | :- |
| <What would be built, and what the gap is today> | <The event that would schedule it> |
```

"When needed" isn't a trigger. "A player reports it on a Wayland-only system" is.

## Decision records

Record each significant or hard-to-reverse decision in the **Decision log**, one architecture
decision record (ADR) each, from this template:

```markdown
## ADR-NNNN — <title>

- **Status:** Proposed | Accepted | Superseded by ADR-NNNN
- **Date:** YYYY-MM-DD
- **Context:** <the forces at play, the constraints, and what prompted the decision>
- **Decision:** <what we will do>
- **Consequences:** <the good and bad results, and the follow-up work>
- **Alternatives considered:** <each option, and why not>
- **Revisit when:** <a concrete, observable trigger>
```

- Give each record the next number, zero-padded to four digits, and append it at the end. Never
  renumber or reorder records.
- Set the status to `Accepted` only when the user made or approved the decision; otherwise
  `Proposed`.
- Keep the context factual and specific: numbers, versions, dates, and the user's choices, with
  the date they were asked.
- When the user chose among options, say so on the decision line itself:
  `**Decision** (the user chose the first two on 2026-10-30):`.
- Split consequences into good and bad, and list the follow-up work with the task or person it
  belongs to.
- Make `Revisit when` an event someone could notice, not "if needed".
- Never edit an accepted record's decision. Supersede it with a new record, and change only the
  old record's `Status` line (`Superseded by ADR-NNNN`, or a short note of what a later record
  adds).
- When a decision changes, update every document that states it (the architecture, the specs,
  the policies, the **Agent guide**), citing the new record in each.
- Cite a record with what it decided, the first time each document mentions it: "ADR-0011
  (movement is scripted, with no physics simulation)".

## Specs

Give each part of the system a spec in the **Specs** folder, in this shape:

```markdown
# Subsystem: `<name>`

<One paragraph: what it is, what it guarantees, and the decision records that shaped it.>

## Why <this design>
<The problem it solves, the alternatives, and what it trades away.>

## <One section per concept: the types, the operations, the formats>

## Invariants
<What always holds, each one testable.>

## Tests
<How the invariants and the behavior are checked.>
```

- Explain why before how. A reader new to the field should understand what problem the design
  solves and what it gives up.
- Sketch types and APIs in code blocks. When the code is built, say so above the sketch ("Built
  in M4.1 as described here"), and add notes where the code settled what the design left open.
- Keep the spec and the code in agreement, and change both in the same change. Where the API
  reference generated from the code is authoritative, the spec summarizes it and links to it.
- Make every invariant testable, and say in `Tests` which test checks it.

## The feature list

When the **Feature list** isn't "none", keep one entry per feature that users would recognize:

```jsonc
{
  "name": "<a few words>",
  "area": "<one of the project's areas>",
  "summary": "<What it does, for a reader who hasn't seen the code, in one to three sentences.>",
  "tasks": ["M3.4", "M3.5"],
  "ticked": ["M3.4"]
}
```

- Every ticked task belongs to a feature. The task that ships something adds it to a feature, or
  describes a new one.
- Write what the ticked tasks built in the present tense, and what is still to come in the
  future tense. Mention future work only from the feature's own `tasks`.
- `ticked` lists the feature's tasks that were ticked when the summary was written. Ticking a
  task means rewriting its feature's summary and updating `ticked` in the same change.

## Status and tense

- Report what is built only in the **Status locations**, and keep them in agreement. Don't add
  status to any other document.
- Write what exists in the present tense, and what doesn't with the task that builds it:
  "`core::path` lands in M1.8". Never describe a command, file, module, or feature as existing
  before it does.
- In the **Diagrams**, draw what exists as built and what is planned as planned, naming the task
  that builds it. When a task builds something drawn as planned, redraw it as built in the same
  change.

## Keeping the documents true

- When a change affects a document, update it in the same change, as the **Update map** lists:
  - a change in behavior or design: its spec;
  - a decision made or changed: a decision record;
  - a roadmap task done: the roadmap, the **Feature list**, and the **Status locations**;
  - a new command, module, or rule: the **Agent guide**;
  - a change to the workflow or the checks: the contribution guide and its templates.
- When you add a document, add it to the **Agent guide**, so agents read it before touching its
  area.
- Keep the shape of the **Checked documents**, and run their checks after editing them.
- Never put a secret in a document or an example, not even one that looks fake.

## Before finishing

1. Run the checks for any **Checked documents** you edited, and the **Feature list** check if you
   ticked a task.
2. Read each changed document as rendered Markdown, and parse any changed diagram or data block.
3. Check that the **Status locations** agree with each other and with the roadmap's ticks.
4. Check that every task ID, decision record, and spec section you cited exists, and that every
   cited record says what you claimed.
5. Do the checks in "Before finishing" in `writing.md`, including the two-reader check.
