---
name: evaluate-dependency
description: Use before proposing any third-party crate or vendored code. Runs the project's build-vs-buy test from docs/02-dependency-policy.md and writes an evaluation for the user to decide on. Never adds the dependency.
argument-hint: "<crate-name> <what it would be used for>"
---

# Evaluate a proposed dependency

Proposal: `$ARGUMENTS`

The project rule: **build it ourselves unless building it is unreasonable.**
You are writing an evaluation for the user to decide on. You never approve
it yourself, and you never change any `Cargo.toml` or `Cargo.lock`.

## Steps

1. **Restate the need.** Describe the capability concretely: inputs,
   outputs, performance, platforms, and where in the architecture it would
   live (which crate and layer).

2. **Try to remove the need.** List design changes that would make the
   dependency unnecessary, each with its cost:
   - a simpler format
   - reduced scope
   - moving the work offline into the cook tool
   - a platform API we already bind

3. **Estimate building it ourselves.**
   - Components and approximate lines of Rust.
   - Specs to implement (with links).
   - The test strategy (vectors, property tests, fuzzing).
   - Risks.
   - Engineer-weeks, as a range, labeled as an estimate.

4. **Apply the exception test.** For each criterion, say "met" or "not met",
   with a reason:
   1. Disproportionate cost: over 4 engineer-weeks, not central, no design
      change avoids it.
   2. Specialist correctness with severe failure modes.
   3. Dev-only tooling with no reasonable equivalent, and never shipped.

5. **Gather facts about the candidate.**
   - Use the web tools if available: crates.io, docs.rs, the repository.
   - Collect:
     - license
     - latest version and release date
     - maintainers and activity
     - transitive dependency count
     - `unsafe` usage
     - MSRV
     - binary-size impact
     - whether it would ship in the game binary
   - **Never add the crate to this workspace to inspect it.** If you
     can't verify a fact, write "unverified".

6. **Write the evaluation** to `docs/dependency-evaluations/<crate>.md`
   using the template below.

7. **Ask the user for a decision.**
   - If they approve, follow the policy's "Exception process" step 3:
     - add a register row
     - write an ADR via `/record-decision`
     - add an allowlist line
     - use an exact version pin
     - wrap the crate behind one module boundary

     Do this as a separate change.
   - If they reject it, record that in the evaluation file, and in the
     register's "Rejected" status if it was listed.

## Template

```markdown
# Dependency evaluation: <crate> (<version>)

- **Date:** YYYY-MM-DD
- **Requested for:** <capability>, used in <crate/layer>
- **Ships in game binary:** yes | no (dev/tool only)

## Need
<concrete description>

## Design changes that would avoid it
| Option | Cost | Viable? |
| :- | :- | :- |

## Build-it-ourselves estimate
<components, specs, test strategy, risks, estimate range>

## Exception test
| Criterion | Met? | Reason |
| :- | :- | :- |
| 1. Disproportionate cost | | |
| 2. Specialist correctness | | |
| 3. Dev-only, no equivalent | | |

## Candidate facts
| Fact | Value | Verified? |
| :- | :- | :- |
| License | | |
| Latest version / date | | |
| Transitive deps | | |
| `unsafe` usage | | |
| MSRV | | |
| Binary size impact | | |

## Recommendation
<build | adopt with conditions | reject>, with one paragraph of reasoning.

## Decision
<left blank for the user>
```
