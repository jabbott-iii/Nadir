---
name: sweep-code
description: Scan the code for bugs and dead code, prove each finding, then fix the approved bugs with regression tests and remove the approved dead code.
argument-hint: "[path, area, or Git range such as v1.2.0..HEAD] [bugs | dead code]"
disable-model-invocation: true
---

# Sweep the code for bugs and dead code

Requested: `$ARGUMENTS` (empty means the whole repository, and both kinds).

This skill looks for two kinds of problems, and fixes the ones the user approves:

- **Bugs:** code that does the wrong thing, or will for some input, timing, or platform.
- **Dead code:** code, files, and dependencies that no build the project ships ever reaches.

It proves each finding before it reports it, changes nothing until the user approves, and makes
each fix a change of its own. Bold names are values from the "Project details" of the adopted
rules:

- **Git access**, **Done checks**, **Never commit**, and **Breaking changes** from `commits.md`;
- **Check commands**, **Test layout**, **Dependency policy**, **Public API**, and
  **Published crates**, **Published modules**, or **Published rocks** from the language rules
  (`rust.md`, `kotlin.md`, `android.md`, `kotlin-multiplatform.md`, `swift.md`, `flutter.md`, and
  `lua.md`), **Exported components** from `android.md`, and **Test tools** from
  `web-front-end.md`;
- the **Docs folder** and the **Decision log** from `roadmap-and-docs.md`, where it is adopted.

Where a value doesn't exist because a rule isn't adopted, work from what the repository has, and
say so in the report. Work with the user as the "Working as a pair" sections of the language
rules describe.

## 1. Set the scope

- **A path or an area:** the code there, its tests, and the files only it uses.
- **A Git range:** the code the range changed (`git diff --stat <range>`) and the code that calls
  it. New bugs most often sit in new code, and new dead code in what the new code replaced.
- **Empty:** the whole repository, one module, crate, or package at a time. If there are more
  than a few, report after each one.
- **"bugs" or "dead code"** after the scope: look for that kind only.

Leave out generated code, vendored third-party code, and the **Never commit** paths. Report a
bug found there, but fix it at its source: the generator's input, or the upstream project.

Run `git status` before you start. If the tree has uncommitted changes you didn't make, leave
them alone and tell the user, because a fix mixed into them can't be reviewed or reverted on its
own. Then tell the user in one or two sentences what you will scan and in what order, and make a
task list whose last item is "Verify and report".

## 2. Record a baseline

Before you change anything, run the **Check commands** (or the **Test tools**) and the
**Done checks**, and record each result: what passes, what fails, and how many warnings each
tool prints. A check that already fails is a finding, not something your work broke. If the code
doesn't build, report that first, and look for findings only by reading until the user decides
what to do.

## 3. Look for bugs

**Run the tools.** Run the project's own checks at the strictest settings it uses, then the tools
in "Tools by language" that the project already has. Read every warning, including the ones the
project has learned to ignore. Don't install a tool, or run a command that downloads one (such
as `npx` or `dart pub global activate`), without asking the user first.

**Question every suppression.** Each one hides a possible finding: `#[allow(…)]`, `@Suppress`,
`@SuppressLint`, a lint baseline, `// ignore:`, `// swiftlint:disable`, `eslint-disable`,
`@ts-ignore`, `# noqa`, `-- luacheck: ignore`, and `---@diagnostic disable`. Check that its reason
still holds. In Rust, `#[expect(…)]` in place of `#[allow(…)]` makes the compiler warn
(`unfulfilled_lint_expectations`) when the lint no longer fires, so it shows which suppressions hide
nothing.

**Read the code.** Look for what tools miss, hardest where input enters, where errors are
handled, and where work runs at the same time:

| Kind | What to look for |
| --- | --- |
| Errors | Errors caught and dropped (an empty `catch`, `let _ =`, `try?`, `.ok()`, `runCatching` with its result ignored); errors logged and then ignored; a crash on bad input (`unwrap`, `!!`, a force unwrap, an unchecked index) |
| Edges | Off-by-one errors; empty and one-item input; integer overflow and narrowing casts; division by zero; NaN (not a number); `==` on floating-point numbers |
| Untrusted input | Sizes and counts used without a limit; path traversal (`../` in a file name); input pasted into SQL, a shell command, or HTML |
| Concurrency | Data races; check-then-act on shared state (acting on a value another thread may have changed since it was read); a lock held across an `await`; work that is never canceled; futures, promises, and tasks never awaited |
| Resources | Files, connections, listeners, subscriptions, and timers never closed or removed; reference cycles that keep objects alive |
| State | A cache that is never invalidated; state left half-updated after a failure; writes that belong in one transaction but aren't |
| Logic | Inverted conditions; the wrong operator; a copied branch with one name left unchanged; a shadowed variable; a `switch` case that falls through; identity compared where equality was meant; in Lua, a method called with `.` instead of `:`, and `#` on a table that can have holes |
| Time and place | Time zones and daylight saving time; clocks that can go backward; parsing and formatting that depend on the device's locale |
| Contracts | Code that disagrees with its doc comment, its spec, its test's name, or the decision record that set it; a `TODO` or `FIXME` that describes a known bug |

**Prove each one.** Give every suspected bug a proof before you report it, and say which kind:

- **Confirmed:** a test or a run fails because of it. Write that test now when you can; it
  becomes the fix's regression test.
- **Traced:** an input, timing, or platform that reaches the bug, followed step by step through
  the code, but not run.
- **Suspected:** a pattern that is often a bug, with no path to it found yet. Report it as a
  question, never as a bug.

## 4. Look for dead code

**What counts:**

- Unreachable code: statements after a `return` or a `throw`, and branches whose condition can't
  be true.
- Unused private and internal declarations: functions, types, fields, constants, parameters,
  imports, and local variables.
- Unused public declarations in an application. A library's are different: see "Libraries"
  below.
- Feature flags that are always on or always off, and the branches they hide.
- Compatibility code the project no longer needs, such as a check for an OS version below the
  lowest one it supports.
- Commented-out code.
- Unused files: resources, assets, strings, styles, scripts, config, and fixtures.
- Unused dependencies, and build targets and scripts that nothing runs.
- Tests of dead code only. They are dead too, and go with it.

**Prove it dead.** A search that finds no caller isn't proof. Before you call something dead,
rule out every way code is reached without a direct call:

- Reflection, serialization, and dependency injection: names in annotations, JSON fields, and
  dependency injection modules.
- Names looked up as text: Android resources by name, platform channel and method names, routes,
  event names, CSS classes that JavaScript builds, registries keyed by strings, and in Lua, module
  names built as strings for `require` and functions looked up by name (`M[name]`).
- Entry points the platform calls: the components in the Android manifest (the
  **Exported components**), `Info.plist` entries on Apple platforms, `main` functions, scheduled
  jobs, command-line subcommands, and the callbacks a Lua host calls by name.
- Symbols exported to another language: `#[no_mangle]` and `extern "C"`, Java Native Interface
  (JNI) functions, `@objc`, `@JvmStatic`, the framework API that Kotlin Multiplatform exports to
  Swift, Dart's `@pragma('vm:entry-point')`, and C functions registered with Lua (a `luaL_Reg`
  array).
- Code that only some builds use: platforms, targets, build variants, and features
  (`#[cfg(…)]`, Cargo features, `expect` and `actual`, `#if os(…)`, Gradle product flavors,
  `kIsWeb`, and Lua's checks of `jit` or `_VERSION`).
- Code generators, build scripts, macros, templates, and other repositories that call or copy
  this code.
- Work the roadmap plans: code written ahead for a planned task isn't dead. Check the roadmap in
  the **Docs folder** and the **Decision log**, and ask the user.

**Libraries.** Code you can't see uses a library's public items, so never call one dead because this
repository doesn't use it. The **Public API** and the **Published crates**, **Published modules**,
or **Published rocks** say what is published. Removing a published item falls under
**Breaking changes** and is the user's decision; suggest deprecating it first.

**Classify each finding:**

- **Dead:** no path reaches it in any build, and the compiler or a tool agrees where one can
  tell.
- **Probably dead:** no use found, but a way in from the list above can't be ruled out. Report it
  with what you checked.

Compilers find only part of the dead code. "Tools by language" says what each one misses.

## 5. Report the findings, then ask

Show every finding in one table before you change anything:

| # | Kind | Where | What's wrong | Proof | Proposed fix | Risk |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | Bug, confirmed | `src/header.rs:88` | A header over 255 bytes wraps its `u8` length to 0 | `rejects_long_header` fails | Reject lengths above 255 | Low: one function |
| 2 | Dead code, dead | `src/legacy.rs` | Nothing has called the v1 parser since v2.0.0 | No reference in any target or feature; the compiler warns | Delete the file and its tests | Low |

List the bugs first, the worst first: data loss and security, then wrong results, then crashes
on rare input. Then the dead code. Then ask the user which findings to fix. Offer to fix every
confirmed bug and every dead private item under one approval, and ask about each of these by its
number:

- a traced or suspected bug;
- a probably-dead item;
- a fix that changes public API, a file format, stored data, or behavior a user can see;
- deleting a whole file, module, or dependency.

## 6. Fix one finding at a time

Make each fix a complete change that one commit can describe.

**A bug:**

1. Write a test that fails because of the bug, where the **Test layout** puts tests. Run it, and
   watch it fail for the reason you expect.
2. Fix the cause, not the symptom: the code that let the bad value in, not the place it crashed.
   Never make a bug go away by suppressing a warning, discarding the error, or loosening a test.
3. Run the test again, and watch it pass.
4. Look for the same mistake elsewhere, because copied code copies its bugs. Add each one you
   find to the findings, and fix it only once the user approves it.

If no test can show the bug, as with a race that rarely happens or a device you don't have, say
so, and tell the user how to confirm the fix.

**Dead code:**

1. Delete all of it: the code, its tests, the docs and comments that mention it, and the
   resources, config, and dependencies only it used. Don't comment it out, rename it `_unused`,
   or hide it behind a flag; Git keeps the history.
2. Remove a dependency with the package manager's own command where it has one (`cargo remove`,
   `npm uninstall`, `dart pub remove`), so the lock file stays right, and follow the
   **Dependency policy**.
3. Leave the code around it as it is: don't refactor, reformat, or rename it in the same change.

Stop and ask if a fix grows bigger than its finding described, or needs a decision the user
hasn't made. If a finding proves wrong while you fix it, drop it, and say so in the report.

## 7. Verify

1. Run the **Check commands** (or the **Test tools**) and the **Done checks** again, and compare
   each result with the baseline: nothing newly failing, and no new warnings.
2. Confirm that each bug's regression test failed before its fix and passes now.
3. After removing dead code, build and test every platform, target, build variant, and feature
   combination the project has, because code that only one of them uses fails only there. For
   example, in Rust run `cargo check --all-targets` with `--all-features` and with
   `--no-default-features`.
4. Search once more for every name you deleted, in the docs, config, and scripts too.
5. Do the checks in "Before finishing" in `writing.md` for each comment and document you
   changed.

## 8. Record what's left

- List the findings you didn't fix, and why: not approved, waiting for a decision, or outside the
  scope.
- If `roadmap-and-docs.md` is adopted, offer to add the ones the user wants to keep to the
  roadmap's backlog. Don't open issues in a tracker unless the user asks.
- For each kind of finding that turned up more than once, suggest a check that would catch it
  next time: a lint to turn on, a stricter compiler setting, a test, or a tool in continuous
  integration (CI). Don't add one unless the user agrees.

## 9. Commit only as the project allows

Follow the **Git access** level in `commits.md`, and leave the changes uncommitted unless the user
asked for a commit and that level allows it. Either way, end the report with copy-ready commit
messages, as `commits.md` describes, one for each fix. A bug fix is a `fix`; removing dead code
without changing behavior is a `refactor`; removing only an unused dependency is a `build`
change.

## 10. Report

End with a summary that contains:

- **Scope:** what you scanned, and what you left out and why.
- **Baseline:** each check's result before you started.
- **Fixed:** each finding fixed, with its regression test, or the proof that the code was dead.
- **Not fixed:** each finding left, by its number, and why.
- **Dropped:** each finding that proved wrong.
- **Checks:** each check after the fixes, with its real result compared with the baseline, and
  each one you couldn't run, and why.
- **Questions:** each decision the user still has to make.
- **Prevention:** the checks you suggest in step 8.
- **Commit messages:** in `text` blocks, as `commits.md` describes, with no attribution lines.

## Tools by language

Use the tools the project already has first. This table says what each compiler reports on its
own, what it misses, and the tools that cover the rest. The compiler columns held for rustc 1.99,
Kotlin 2.4.21, Swift 6.4.3, Dart 3.14.1, TypeScript 7.0.2, Lua 5.1.5, 5.4.9, and 5.5.1, and
LuaJIT 2.1.

| Language | What the compiler reports | What it misses, and the tools that find it |
| --- | --- | --- |
| Rust | `dead_code` for items nothing in the crate uses, and `unused_imports`, `unused_variables`, and `unreachable_code`. An item that only tests use counts as unused in `cargo check`. | The `pub` items a library exports. Clippy (`cargo clippy --all-targets --all-features`) for likely bugs; Miri (`cargo +nightly miri test`) for undefined behavior in `unsafe` code, though it can't run most foreign function interface (FFI) calls; `cargo machete` (fast but imprecise) or `cargo +nightly udeps` for unused dependencies; `cargo audit` or `cargo deny check advisories` for dependencies with known vulnerabilities |
| Kotlin and Android | With `-Wextra` only: unused local variables and unreachable code. The experimental `-Xreturn-value-checker=full` reports ignored return values. | Unused private functions, properties, and parameters, and unused imports: detekt's `Unused…` rules in its style rule set, some of them off by default. Android Lint's `UnusedResources`, and `ObsoleteSdkInt` for version checks that `minSdk` makes always true. The Dependency Analysis Gradle Plugin's `buildHealth` task for unused dependencies |
| Swift | Unused local constants and variables, ignored results of functions without `@discardableResult`, and, in a full build but not with `-typecheck`, code after a `return` | Unused functions, types, properties, and parameters: Periphery (`periphery scan`), now a commercial product, which on Linux scans only Swift packages. The sanitizers for bugs: `swift test --sanitize=thread`, `address`, or `undefined` |
| Dart and Flutter | `unused_import`, `unused_local_variable`, `unused_field`, `dead_code`, and `unused_element` for private declarations (names that start with `_`) | Unused public declarations, which no analyzer diagnostic reports: search for them, and rule out the ways in above. `dependency_validator` for unused packages |
| JavaScript and TypeScript | TypeScript with `noUnusedLocals`, `noUnusedParameters`, `allowUnreachableCode: false`, and `noFallthroughCasesInSwitch`; ESLint's `no-unused-vars`, `no-unreachable`, and `no-fallthrough`, which its recommended config turns on | Unused exports, files, and dependencies: knip. CSS and JavaScript a page never runs: the **Coverage** panel in Chrome DevTools. `npm audit` for dependencies with known vulnerabilities |
| Python | No compile step reports unused code | Ruff's `F401` (unused imports) and `F841` (unused local variables), which it checks by default; Vulture for unused functions, classes, and variables, with `--min-confidence`, and a `--make-whitelist` file for its false positives |
| Lua | Nothing: `luac -p` and LuaJIT's compiler check syntax only. The exception is Lua 5.5: after a `global` declaration, an undeclared name, such as a misspelled global, is a compile error. | luacheck's warnings 211 (unused local variable), 212 (unused argument), 213 (unused loop variable), 311 (a value assigned to a local is never used), 511 (unreachable code), and 111 and 113 (setting and reading an undefined global). lua-language-server's `unused` diagnostics, such as `unused-local`, `unused-function`, and `unreachable-code`. selene's `unused_variable`. Each checks one file at a time, so finding a module or a public function that no other file uses takes a search |
