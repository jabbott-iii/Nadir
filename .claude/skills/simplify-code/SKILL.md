---
name: simplify-code
description: Comb the code for functions and modules that are longer or more convoluted than needed, and simplify the approved ones without losing any functionality.
argument-hint: "[path, area, or Git range such as v1.2.0..HEAD; empty means the whole repository]"
disable-model-invocation: true
---

# Simplify long or convoluted code

Requested scope: `$ARGUMENTS` (empty means the whole repository).

This skill finds code that is longer or harder to follow than its job requires, and rewrites the
parts the user approves so that they do exactly what they did before, in a form that is easier to
read and to change. Simpler means fewer things a reader must hold in mind at once: less nesting,
fewer branches and special cases, fewer layers, and names that say what the code does. It
doesn't mean fewer characters, and it never means less functionality. For bugs and dead code,
use `/sweep-code` where the project has adopted it.

Bold names are values from the "Project details" of the adopted rules:

- **Git access**, **Done checks**, **Never commit**, and **Breaking changes** from `commits.md`;
- **Check commands**, **Test layout**, **Public API**, and **Published crates**,
  **Published modules**, or **Published rocks** from the language rules (`rust.md`, `kotlin.md`,
  `android.md`, `kotlin-multiplatform.md`, `swift.md`, `flutter.md`, and `lua.md`), with their
  **Lint configuration** or **Style and lint configuration**, and their **Design docs**;
  **Performance budgets** from `lua.md`; and **Test tools** and **Browser support** from
  `web-front-end.md`;
- the **Docs folder** and the **Decision log** from `roadmap-and-docs.md`, where it is adopted.

Where a value doesn't exist because a rule isn't adopted, work from what the repository has, and
say so in the report. Work with the user as the "Working as a pair" sections of the language
rules describe.

## Ground rules

These hold for the whole run, and override anything a later step seems to allow:

- **Never give up functionality.** A simplification keeps everything the code does:
  - every input it accepts, and how it handles each one, including empty, missing, and invalid
    input;
  - every output, return value, and error, with its type and message;
  - every side effect, in the same order: writes, network calls, events, and logs;
  - every edge case, check, and validation, and every retry, timeout, and limit;
  - every option and setting, and every platform, version, and browser it supports (the
    **Browser support**, for a website);
  - its accessibility, its translations, and its thread safety;
  - and its speed and memory use, within the **Performance budgets** and any other budget the
    project sets.

  If the only simpler form drops any of these, it isn't a simplification. Leave the code as it is,
  or describe the trade-off and let the user decide, as a change separate from this run.
- **Prove that the behavior stays the same.** Tests must cover the code before you change it,
  and the same tests must pass after. Where they don't cover it, first write tests that pin down
  what it does now.
- **Keep the public API.** The **Public API** and the **Published crates**, **Published modules**,
  or **Published rocks** say what other code depends on. Never change a public signature, a file
  format, a protocol, or a name that other code calls, unless the user approves it under
  **Breaking changes**.
- **Don't fix while you simplify.** If the code looks wrong, report it and keep its current
  behavior. A fix is a change of its own, with its own test.
- **Match the codebase.** Use the idioms the project already uses. Don't bring in a library, a
  pattern, or a language feature the project doesn't use only to make one function shorter.

## 1. Set the scope

- **A path or an area:** the code there, and the tests that cover it.
- **A Git range:** the functions the range changed (`git diff --stat <range>`), because new code
  is where needless complexity is cheapest to remove.
- **Empty:** the whole repository, one module, crate, or package at a time, starting with the
  places step 3 ranks highest. If there are more than a few, report after each one.

Leave out generated code, vendored third-party code, and the **Never commit** paths. Run
`git status` before you start. If the tree has uncommitted changes you didn't make, leave them
alone and tell the user. Then tell the user in one or two sentences what you will look at and in
what order, and make a task list whose last item is "Verify and report".

## 2. Record a baseline

Before you change anything, record what you will compare against later:

- the result of each of the **Check commands** (or the **Test tools**) and the **Done checks**;
- the findings of the length and complexity lints at the project's own thresholds, from the
  **Lint configuration** or **Style and lint configuration**, for each function in scope;
- test coverage of the code in scope, where the project measures it;
- and, for code under a performance budget, a benchmark, run several times so you know its noise.

## 3. Find the candidates

**Rank where to look.** Simplifying code that is rarely read or changed pays back little. Start
where code is both complicated and changed often. This lists the files changed most often in the
past year:

```bash
git log --since=1.year --format= --name-only | grep . | sort | uniq -c | sort -rn | head -20
```

**Run the tools.** Run the length, complexity, and duplication tools the project has (see "Tools
by language") at its thresholds, then at the tools' defaults for a wider look. A score is a
reason to look, not proof that code is too complex. Two scores are common:

- **Cyclomatic complexity** counts the independent paths through a function: one, plus one for
  each branch.
- **Cognitive complexity** estimates how hard a function is to read. It ignores shorthand that
  reads easily, adds one for each break in the linear flow, and adds more when those breaks are
  nested.

**Read the code** for the patterns tools miss. Each has a simpler form, and a condition for
keeping every behavior while you use it:

| Pattern | Simpler form | To keep every behavior |
| --- | --- | --- |
| Deep nesting | Guard clauses that return early, so the main path runs at the left margin | Keep each check, its order, and what each early return returns or raises |
| A long function that does several things | Smaller functions, each named for one step | Move code without reordering it; extract only steps that deserve a name of their own |
| A long `if`/`else` chain or `switch` that maps data to data | A lookup table or map | Keep the default case, and the same result for every key, including unexpected ones |
| A complicated condition | A variable or function whose name says what the condition means | Keep the order of short-circuit operators where one part guards the next (`user != null && user.isAdmin`) or has a side effect |
| A Boolean flag parameter | Two functions, or an enumeration | Keep every caller's behavior, and the default |
| A long parameter list | A parameter object or an options table | Keep every default and every validation |
| A hand-written loop that redoes a library function | The standard library's version (`map`, `filter`, `any`, `sum`, `join`, `table.concat`) | Keep early exits, the order of side effects, lazy or eager evaluation, and the result for empty input |
| The same code in several places | One shared function, once the copies must change together | Diff the copies first: copies that only look alike stay apart, and real differences become parameters |
| An interface, factory, wrapper, or layer with one implementation that only forwards calls | Call the implementation directly | Check the roadmap and the **Decision log** for a planned second implementation, and keep any seam that tests use to substitute a fake |
| Clever code: nested ternaries, dense one-liners, bit tricks | Plain statements | Keep the exact result, including on overflow, NaN (not a number), and `nil` or `null` |
| A redundant check or conversion, or `if x then return true else return false` | Remove it, or `return x` | Remove a check only when something else enforces it in every build, release builds included |
| A flag variable that steers a loop, or a variable set once and used far away | `break` or `return`; the expression where it's used | Keep when each value is computed, if it can change in between |
| Error handling that only rethrows, or wraps and loses information | Let the error propagate (`?` in Rust) | Keep the error's type, message, and context that callers and logs rely on |
| Comments that restate the code | Delete them, and keep the ones that say why | Keep every comment a tool reads, such as a lint suppression or a doc annotation |

**Find out why it's long before you call it needless.** Read the history of the lines,
`git log -L <start>,<end>:<file>`, and the issue, pull request, or decision record behind them.
Much length is functionality:

- **Rules of the domain:** tax rules, protocol states, date and time zone handling, and every
  case a spec lists. These can be organized, but not removed.
- **Performance:** a hot path written out by hand on purpose, as a comment or a benchmark says.
- **Compatibility:** branches for the platforms, versions, and browsers the project still
  supports.
- **Robustness:** validation, retries, timeouts, and defensive checks where untrusted data comes
  in.
- **Planned work:** an extension point a task in the roadmap in the **Docs folder** will use.

If the reason still holds, leave the code as it is, and suggest a comment that records the reason
where there isn't one.

## 4. Report, then ask

Show every candidate in one table before you change anything:

| # | Where | What makes it hard to follow | Measure now | Proposed simplification | What it keeps | Tests | Risk |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `apply_discounts` in `src/billing.rs:120` | Five levels of nesting | Cognitive complexity 31 | Guard clauses for the three invalid cases, then the main path | Every check in its order, and every error | Every branch covered | Low |
| 2 | Seven copies of a retry loop in `src/sync/` | The same 20 lines, changed by hand in each place | 140 lines | One `with_retry` function | The same attempts, delays, and errors; two copies log different messages, so the message becomes a parameter | Four of seven covered; tests for the other three come first | Medium |

Then ask the user which to make. Offer every low-risk one under one approval: a change inside one
function, covered by tests, that changes nothing public. Ask about each of these by its number:

- a change across several files or modules;
- a change to anything public, under **Breaking changes**;
- removing an abstraction;
- a change to code under a performance budget;
- a change that needs new tests first;
- and any change where you are less than sure about what it keeps.

## 5. Simplify one item at a time

Make each simplification a complete change that one commit can describe.

1. **Pin the behavior.** Confirm that tests cover every path the change touches, where the
   **Test layout** puts tests. Add the missing tests first, and run them against the old code:
   they must pass there. A new test that fails on the old code means you misread what it does.
2. **Change in small steps,** one move at a time (extract a function, inline one, rename, turn a
   nested `if` into a guard clause), and run the tests after each step.
3. **Don't edit a test to make it pass.** Change a test only when it reaches into a private
   detail the simplification removed, and then only so that it checks the same behavior through
   the public path. Say so in the report.
4. **Keep the words true.** Update doc comments and the **Design docs** the change affects, and
   delete comments the new code makes unnecessary.
5. **Run the formatter,** and revert its changes to code you didn't otherwise touch.

Stop and ask if a step needs a decision the user hasn't made, or turns out to change behavior. If
a simplification doesn't make the code clearer after all, drop it, and say so in the report.

## 6. Verify

1. Run the **Check commands** (or the **Test tools**) and the **Done checks** again, and compare
   each result with the baseline: nothing newly failing, no new warnings, and the tests from
   before passing unchanged.
2. Go through the "What it keeps" column of each item you changed, and confirm each behavior in
   the new code and the tests.
3. Run the length and complexity tools again, and compare each function with the baseline. Give a
   reason for any item whose measure didn't improve.
4. For code under a performance budget, run the benchmark again, several times: it must stay
   within the budget and match the baseline within its noise.
5. Build and test every platform, target, build variant, and feature combination the project
   has, because a change can break code that only one of them compiles.
6. Read each changed function once as someone new to the code would. The goal is code that is
   easier to follow, not code that is shorter.
7. Do the checks in "Before finishing" in `writing.md` for each comment and document you changed.

## 7. Record what's left

- List the candidates you didn't change, and why: not approved, waiting for a decision, length
  that is functionality, or outside the scope.
- For each kind of complexity that turned up more than once, suggest a check that would catch it
  next time: a lint to turn on, or a threshold to set in continuous integration (CI). Don't add
  one unless the user agrees.

## 8. Commit only as the project allows

Follow the **Git access** level in `commits.md`, and leave the changes uncommitted unless the user
asked for a commit and that level allows it. Either way, end the report with copy-ready commit
messages, as `commits.md` describes, one for each simplification, of type `refactor`. Put the
tests that pin the old behavior in a `test` commit before each one, so a reviewer can see them
pass on both the old code and the new.

## 9. Report

End with a summary that contains:

- **Scope:** what you looked at, and what you left out and why.
- **Baseline:** each check's result and each measure before you started.
- **Simplified:** each item, with its measure before and after, and what it keeps.
- **Left as is:** each candidate you didn't change, and why, including length that is
  functionality.
- **Dropped:** each simplification that didn't make the code clearer.
- **Found along the way:** code that looks wrong, reported but not changed.
- **Checks:** each check after the changes, with its real result compared with the baseline, and
  each one you couldn't run, and why.
- **Questions:** each decision the user still has to make.
- **Prevention:** the checks you suggest in step 7.
- **Commit messages:** in `text` blocks, as `commits.md` describes, with no attribution lines.

## Tools by language

Use the tools the project already has first. The Clippy findings were checked with Clippy 0.1.99;
the rest come from each tool's documentation.

| Language | Length, complexity, and simpler forms |
| --- | --- |
| Rust | Clippy, by default: `too_many_arguments` (more than 7), and lints for simpler forms such as `collapsible_if`, `needless_bool`, `needless_return`, and `manual_map`. Off by default: `too_many_lines` (in the pedantic group, over 100 lines); `cognitive_complexity` (in the restriction group, threshold 25, which its docs say isn't a true Cognitive Complexity measure); and `excessive_nesting`, which fires only once `excessive-nesting-threshold` is set |
| Kotlin and Android | detekt's complexity rule set: `CyclomaticComplexMethod`, `LongMethod`, `LongParameterList`, `NestedBlockDepth`, `ComplexCondition`, `LargeClass`, and `TooManyFunctions` are on by default, and `CognitiveComplexMethod` is off. Detekt 2.0 renamed every threshold option and changed some defaults (`LongParameterList` allows 5 function parameters instead of 6) |
| Swift | SwiftLint, on by default, with warning and error thresholds: `cyclomatic_complexity` (10, 20), `function_body_length` (50, 100 lines), `type_body_length` (250, 350), `file_length` (400, 1000), `function_parameter_count` (5, 8), and `nesting`. `closure_body_length` is opt-in |
| Dart and Flutter | Dart's own linter has no rule for complexity, length, nesting, or parameter counts; `avoid_positional_boolean_parameters` catches flag parameters. DCM, a commercial tool with a free tier, reports metrics such as cyclomatic complexity and maximum nesting level |
| JavaScript and TypeScript | ESLint's core rules, none of them in its recommended config: `complexity` (default maximum 20), `max-depth` (4), `max-lines-per-function` (50), `max-params` (3), `max-statements` (10), and `max-nested-callbacks` (10), plus the frozen style rules `no-nested-ternary`, `no-else-return`, and `no-lonely-if` |
| Python | Ruff: `C901` (McCabe complexity over `max-complexity`, default 10), `PLR0911`, `PLR0912`, `PLR0913`, and `PLR0915` (too many returns, branches, arguments, and statements), none of them on by default; of the flake8-simplify `SIM` rules, `SIM102` (collapsible `if`) is on by default |
| Lua | luacheck's `max_cyclomatic_complexity` option, off by default, reports warning 561 for a function over the limit |
| Any language | Duplicated code: jscpd (`npx jscpd <path>`) and PMD's CPD (`pmd cpd --minimum-tokens 100 --dir <path>`), which both cover every language in this table |
