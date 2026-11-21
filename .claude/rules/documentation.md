---
paths:
  - "**/*.md"
  - "docs/**"
  - "diagrams/**"
  - "site/**"
  - "**/*.rs"
---

# Rules for writing documentation

These rules apply to everything written for people to read: `docs/`, the
READMEs, `AGENTS.md`, `CLAUDE.md`, `CONTRIBUTING.md`, the diagrams, the
demo page, the feature summaries in `docs/features.jsonc`, and Rust doc
comments (`///` and `//!`, which `cargo doc` turns into the API
reference). `CONTRIBUTING.md` → "Documentation" says which docs a change
must update; this file says how to write them.

Write for two readers at once: a second-year computer science student new
to the engine and a senior engineer who needs every detail.

## Keep jargon down

- **Use a plain phrase** whenever it is just as exact.
- **Use a technical term only** when it carries meaning a plain phrase
  can't, or when the reader needs the term to search for it or use the
  tool.
- **Explain each term a student might not know** the first time it
  appears, in a short clause or parenthetical. Then use it freely. For
  example: "y-sorted (drawn in order of their y position, so a sprite
  lower on screen covers one above it)".
- **Spell out acronyms on first use,** as in "render hardware interface
  (RHI)". Universal ones such as HTML and API don't need it.
- **Drop insider shorthand:**
  - slang such as "footgun" and "yak shave"
  - a bare reference to an architecture decision record or a roadmap
    task, such as "ADR-0011" or "M8.2", that doesn't say what the record
    decided or what the task builds. Write "ADR-0011 (movement is
    scripted, with no physics simulation)".

## Don't give up meaning

- **Accuracy wins over simplicity.** Keep the exact numbers, limits,
  terms, and caveats.
- **If a simpler sentence would be wrong or misleading,** keep the precise
  wording and add an explanation beside it.
- **Don't remove details senior engineers need.** Explain them so
  everyone else can follow.

## Help newer readers follow

- **Explain why** a design choice was made, not only what it is.
- **Introduce one idea at a time,** with concrete examples. An analogy
  goes next to the real explanation, never in place of it.
- **Leave out "obviously" and "trivially".**
- **Keep paragraphs short,** and write headings that make the page easy
  to skim: "Why sprites are y-sorted", not "Notes".

## In doc comments

A reader lands on one item's page, not at the top of the crate. Explain
a term once, in the module docs (`//!`) or on the item that defines it.
Elsewhere, link to that explanation with an intra-doc link such as
``[`World`]`` instead of repeating it on every item.

## Use American English

- **Spelling:**
  - -or, not -our: color, behavior, neighbor, honor
  - -er, not -re: center, centered
  - -ize, not -ise, where British English uses -ise: initialize,
    serialize, normalize
  - one l before -ed and -ing when the word's last syllable isn't
    stressed: canceled, labeled, modeling, tunneling (but controlled and
    compelled keep two)
  - and gray, catalog, spelled (not spelt), acknowledgment, and license
    as both noun and verb
- **Grammar:** write complete sentences whose subjects and verbs agree.
  Keep one tense for the same kind of statement.
- **Punctuation:** use the serial comma (the comma before the final "and"
  or "or" in a list of three or more), as in "numbers, limits, and
  caveats". Most existing pages leave it out; add it in the sentences you
  write or change.
- **Quote names exactly:** code identifiers, file names, the fields of
  APIs and file formats, command output, and third-party names keep their
  own spelling, even when it is British.
- **Fix only the text you change.** Don't sweep a whole page for spelling
  or commas as part of another task (`AGENTS.md`, rule 6: the smallest
  complete change). Mention the ones you notice in your summary instead.

## Before finishing: the two-reader check

1. Reread the draft as a second-year computer science student. Look for
   undefined terms and skipped steps.
2. Reread it as a senior engineer. Look for anything vague, oversimplified,
   or wrong.
