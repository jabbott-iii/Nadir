---
name: fix-doc-drift
description: Find statements in the project's documentation that no longer match the code, the tools, or the project's own records, and correct them, asking first wherever the documentation may be right and the code wrong.
argument-hint: "[path, area, or Git range such as v1.2.0..HEAD; empty means every document]"
disable-model-invocation: true
---

# Correct documentation drift

Requested scope: `$ARGUMENTS` (empty means every document).

Documentation drifts when the thing it describes changes and the text doesn't. This skill finds
the statements that are no longer true and corrects them. Bold names such as
**Sources of truth** are values from the "Project details" of the rules this project has
adopted: `writing.md` (required), `roadmap-and-docs.md` (where adopted), and `commits.md`. The
language rules add the checks for doc comments and examples. Where a value doesn't exist because
a rule isn't adopted, work from what the repository has, and say so in the report.

## 1. Set the scope

- **A path or an area:** the documents there, and the doc comments in that area's code.
- **A Git range:** run `git log --stat` over the range to see what changed. Check the documents
  that describe those changes, using the **Update map** where it exists, and every one of the
  **Status locations**.
- **Empty:** every place in the **Where text lives**. Start with the **Status locations** and the
  **Agent guide**, because drift there misleads people and agents the most.

Tell the user in one or two sentences what you will check and in what order. Then make a task
list whose last item is "Verify and report". If the scope holds more than about 30 documents,
work in batches and report after each one.

## 2. Collect the claims that can drift

In each document, list the statements that a source can confirm or refute:

| Claim | Example | Checked against |
| --- | --- | --- |
| Status | "is built", "lands in M3.4", a ticked task | The roadmap's ticks, and the code |
| Commands and flags | `cargo xtask ci --fix` | The tool's real `--help` or output |
| Paths and file names | `docs/subsystems/ecs.md` | The repository's files |
| Names from the code | Types, functions, config keys, environment variables | The code, or its generated API reference |
| Numbers | Versions, counts, limits, defaults, and measured results | Manifests, constants in the code, and recorded measurements |
| References | Task IDs, decision records, spec sections, and links | The roadmap, the **Decision log**, the specs, and the links themselves |
| Examples | A code block in a README | Compiling or running it |
| Decisions restated elsewhere | "There are no third-party crates" | The decision log's current record |
| Diagrams | A part drawn as built or as planned | The code and the roadmap |

Leave out what records the past: decision records' context, changelogs, errata, dated notes, and
statements of why something was chosen. They aren't drift when the present changes.

## 3. Check each claim against its source

- Use the **Sources of truth**. The code settles what the code does, the roadmap's ticks settle
  what is built, the decision log's current record settles what was decided, and a tool's real
  output settles its commands and flags.
- Search the code for every name and path. Run commands only in ways that change nothing:
  `--help`, `--version`, a dry run, or a check. Never run a command that writes files,
  publishes, or deploys only to confirm a document.
- Build the docs and run the examples the way the language rules describe (for example, doc
  tests, Dokka, or DocC), so a broken example or link shows itself.
- Check that every internal link and anchor resolves. List external links for the user to check
  if this session can't reach them.
- Don't rerun benchmarks or long measurements unless the user asks. Compare a stated number with
  the recorded result instead, and mark it unverifiable if there is none.
- Mark each claim as **matches**, **drifted** (with the source that says otherwise), or
  **unverifiable** (with the reason).

## 4. Decide which side is wrong

Drift runs in two directions. Before you edit a document, decide whether the document is wrong or
the thing it describes is:

- **The document describes the code, and the code is right:** correct the document.
- **The document is a spec, a decision, or a requirement, and the code departs from it:** this is
  a bug or an unrecorded decision, not drift. Don't change the document to match the code.
  Describe the conflict, give the options, recommend one, and ask the user which is intended.
- **You can't tell:** ask.

This skill corrects documents only. Report code that seems wrong instead of changing it.

## 5. Correct the documents

- Make the smallest correction that makes each statement true, and keep the rest of the author's
  structure and wording. Follow `writing.md`.
- Write what exists in the present tense, and what doesn't with the task that builds it, as
  `roadmap-and-docs.md` describes.
- Respect the records of the past:
  - Never rewrite an accepted decision record. If its decision no longer holds, draft a new
    record that supersedes it, as `Proposed`, and ask the user.
  - Never rewrite a finished roadmap task. Add a dated `Note:` under it instead.
  - Leave changelogs, errata, and dated notes as they are.
- Regenerate generated files with their tools instead of editing them by hand.
- Fix every copy of a drifted fact in the scope. Where the same fact is repeated, prefer pointing
  to its one source over correcting each copy, and mention copies outside the scope.
- Keep the **Status locations** in agreement with each other.
- Leave an unverifiable claim as it is; don't guess. List it in the report.
- Don't document behavior that has no documentation yet. List it as missing in the report, and
  write it only if the user asks.

Stop and ask before you delete a document or a section, change what a spec says should happen,
supersede a decision, or change anything outside the scope.

## 6. Verify

1. Check every corrected claim against its source again.
2. Run the checks for any **Checked documents** you edited, and the **Feature list** check if you
   changed it.
3. Build the docs and examples again, and preview the changed documents with the
   **Preview command**.
4. Do the checks in "Before finishing" in `writing.md`.

## 7. Suggest how to prevent it

For each kind of drift you found more than once, suggest a check that would catch it next time:
a doc test for an example, a link checker in continuous integration (CI), a script that compares
a table with the code, or a `--check` mode for a generated file. Don't add a check unless the user
agrees.

## 8. Commit only as the project allows

Follow the **Git access** level in `commits.md`, and leave the changes uncommitted unless the user
asked for a commit and that level allows it. Either way, end the report with copy-ready commit
messages, as `commits.md` describes: one commit for each drift you corrected, unless several
corrections fix the same drift.

## 9. Report

End with a summary that contains:

- **Scope:** what you checked, and what you left out and why.
- **Corrected:** a table of each document and line, the old statement, the new one, and the
  source that settles it.
- **Questions:** each conflict where the document may be right and the code wrong, with the
  options and your recommendation.
- **Unverifiable:** each claim you couldn't check, and why.
- **Missing:** behavior that exists but has no documentation.
- **Checks:** each check you ran, with its real result, and each one you didn't run, and why.
- **Prevention:** the checks you suggest from step 7.
- **Commit messages:** in `text` blocks, as `commits.md` describes, with no attribution lines.
