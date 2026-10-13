---
name: record-decision
description: Append an architecture decision record to docs/decisions.md when a design decision is made, changed, or superseded, and update the docs that reference it.
argument-hint: "<short decision title>"
---

# Record an architecture decision

Title: `$ARGUMENTS`

1. Read `docs/decisions.md`. The next number is the highest `ADR-NNNN` plus
   one, zero-padded to four digits.
2. Draft the ADR using the template at the top of that file:
   - **Status:** `Accepted` only if the user explicitly made or approved
     the decision in this conversation. Otherwise `Proposed`.
   - **Date:** today's date.
   - **Context, Decision, Consequences, Alternatives considered:** keep it
     factual and specific. State numbers where they exist.
   - **Revisit when:** must be a concrete, observable trigger, not
     "if needed".
3. **If this supersedes an earlier ADR:**
   - Change only that ADR's `Status` line to `Superseded by ADR-NNNN`.
   - Do not rewrite its text. History stays intact.
4. Append the new ADR at the end of `docs/decisions.md`.
5. Update every doc that states the old decision: architecture, subsystem
   specs, `docs/02-dependency-policy.md`, `AGENTS.md`. Reference the new ADR
   number in each place you change.
6. Report the ADR number and title, its status, and the list of docs
   updated. If it's `Proposed`, say what the user needs to decide.
