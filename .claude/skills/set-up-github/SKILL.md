---
name: set-up-github
description: "Set up a repository's GitHub files: CodeQL, Dependabot, SECURITY.md, CONTRIBUTING.md, issue and pull request templates, CODEOWNERS, and a .gitignore."
argument-hint: "[pieces, such as \"codeql dependabot gitignore\"; empty means all eight]"
disable-model-invocation: true
---

# Set up a repository on GitHub

Requested: `$ARGUMENTS` (empty means all eight pieces).

This skill writes the files GitHub reads to scan a repository for vulnerabilities, keep its
dependencies current, route security reports, guide contributors, and request reviews. It also
writes the `.gitignore` that keeps build output, local settings, and secrets out of Git. It fits
each file to the project: the languages it finds, the checks its rules name, and the people and
promises the user gives it. Run it again later, and it updates the files instead of replacing them.

| Piece | Argument | Files | What it does |
| --- | --- | --- | --- |
| Code scanning | `codeql` | `.github/workflows/codeql.yml` | Runs CodeQL, GitHub's static analyzer, on pushes, pull requests, and a weekly schedule |
| Dependency updates | `dependabot` | `.github/dependabot.yml` | Has Dependabot open pull requests that update dependencies and pinned actions |
| Security policy | `security` | `SECURITY.md` | Tells people how to report a vulnerability privately, and what happens next |
| Contributor guide | `contributing` | `CONTRIBUTING.md` | Tells contributors how to set up, check, commit, and propose a change |
| Issue forms | `issues` | `.github/ISSUE_TEMPLATE/` | Asks reporters for what each kind of issue needs, and sends security reports elsewhere |
| Pull request template | `pr` | `.github/pull_request_template.md` | Starts each pull request's description with what reviewers need |
| Code owners | `codeowners` | `.github/CODEOWNERS` | Requests a review from the people responsible for each path |
| Ignore rules | `gitignore` | `.gitignore` | Keeps build output, local settings, and secrets out of Git, for each language in use |

A file only describes what GitHub should do. Several pieces also need a repository setting that
only an administrator can change, such as private vulnerability reporting, Dependabot alerts, or a
rule that requires code owners' approval. This skill never changes a setting: it lists each one for
the user, with the steps (step 11).

Bold names are values from the "Project details" of the adopted rules:

- the **Preview command** from `writing.md`;
- **Git access**, **Done checks**, **Never commit**, **Push effect**, **Scopes**, **Task IDs**,
  **Changes that travel together**, and **Breaking changes** from `commits.md`;
- **Toolchain**, **Check commands**, **Dependency policy**, and **Test layout** from the language
  rules (`rust.md`, `kotlin.md`, `android.md`, `kotlin-multiplatform.md`, `swift.md`, `flutter.md`,
  and `lua.md`), with **Lua version** in place of **Toolchain** in `lua.md`; **Untrusted input**
  from `rust.md`, `kotlin.md`, and `lua.md`; **Release** from the mobile rules; **Platforms** and
  **Code generation** from `flutter.md`; **Targets** from `kotlin-multiplatform.md`;
  **C boundary** from `lua.md`; and **Build and dependencies**, **Browser support**, and
  **Test tools** from `web-front-end.md`;
- **Check commands**, **Assessment**, and **Errata** from `teaching-materials.md`;
- the **Update map** and **Agent guide** from `roadmap-and-docs.md`, where it is adopted.

Where a value doesn't exist because a rule isn't adopted, work from what the repository has, and
say so in the report. `writing.md` governs the text of every file this skill writes, and
`roadmap-and-docs.md`, where adopted, also governs `CONTRIBUTING.md`.

## Ground rules

These hold for the whole run, and override anything a later step seems to allow:

- **Change files, never settings.** Write and edit files in the working tree only. Never change a
  repository, organization, or account setting, through the web, `gh`, or the API: not a security
  feature, a branch rule, a label, or the default permissions of workflows. Read-only `gh` calls,
  such as those in step 2, are fine where `gh` is signed in.
- **Never invent a person or a promise.** Code owners, a security contact, supported versions,
  response times, rewards for reports, contribution terms, and a code of conduct come from the user
  or the repository's own records. Never take a name, a handle, or an email address from
  `git log` for any of them: a commit's author may have no access to the repository, and an
  address in the history is personal data. Where the user hasn't decided, leave the section out
  and list it in the report rather than write a guess.
- **Merge, never replace.** Read every existing file first, keep what the user wrote, and add only
  what is missing. Show each line you would change or remove, with the reason, and ask first.
- **Never weaken a control.** Don't remove an ignore rule that keeps a secret out, drop a language
  or a query suite the project already scans, widen a workflow's `permissions`, or trigger a
  workflow on `pull_request_target`, which runs with the base repository's secrets.
- **Pin what runs in continuous integration (CI).** Reference every action in a workflow by the
  full-length commit SHA (the commit's 40-character hash) of a release you looked up in this
  session, with the release's tag in a comment. Never copy a SHA from memory, an example, or
  another repository.
- **Keep vulnerability reports out of public issues.** No form, template, or guide may ask for the
  details of a vulnerability in public.
- **Make only read-only network calls:** looking up actions' releases, and reading the
  repository's details with `gh`. Ask before any other, and before you install a tool.

## 1. Set the scope

- The arguments name pieces by the **Argument** column, such as `codeql dependabot`. Empty, or
  `all`, means all eight.
- Run `git status` before you start. If the tree has uncommitted changes you didn't make, leave
  them alone and tell the user.
- Tell the user in one or two sentences which pieces you will set up and in what order, and make
  a task list whose last item is "Verify and report". Do the ignore rules first when they are in
  scope, because every later `git status` depends on them.

## 2. Survey the repository

1. **Read the adopted rules** in `.claude/rules/`, and note each value this run needs.
2. **Find the languages and package managers** from the manifest files that `git ls-files`
   lists, leaving out vendored code and the **Never commit** paths. This table maps each one to its
   language rule, its CodeQL language, and its Dependabot ecosystem:

   | Found | Language rule | CodeQL `language` | Dependabot `package-ecosystem` |
   | --- | --- | --- | --- |
   | `Cargo.toml` | `rust.md` | `rust` | `cargo`, and `rust-toolchain` for a `rust-toolchain.toml` |
   | `build.gradle.kts` or `build.gradle` | `kotlin.md`, `android.md`, `kotlin-multiplatform.md` | `java-kotlin` | `gradle` |
   | `Package.swift`, or an `.xcodeproj` with Swift files | `swift.md`, and the iOS app in `kotlin-multiplatform.md` | `swift` | `swift`, which reads `Package.swift` |
   | `pubspec.yaml` | `flutter.md` | none: CodeQL doesn't analyze Dart. The app's Kotlin and Swift code count as those languages | `pub` |
   | `*.rockspec`, or `.lua` files | `lua.md` | none for Lua. C modules at the **C boundary** are `c-cpp` | none: Dependabot doesn't list LuaRocks among its ecosystems |
   | `package.json` | `web-front-end.md` | `javascript-typescript` | `npm`, which also covers pnpm and Yarn, or `bun` |
   | `Gemfile` | `web-front-end.md`, for a Jekyll site | `ruby` | `bundler` |
   | `requirements.txt` or `pyproject.toml` | `teaching-materials.md`, for a notebook environment | `python` | `pip`, which also covers Pipenv, pip-compile, and Poetry, or `uv` for a `uv.lock` |
   | `.github/workflows/*.yml` | none | `actions` | `github-actions`, with `directory: "/"` |
   | `Dockerfile` | none | none | `docker` |
   | `.gitmodules` | none | none | `gitsubmodule` |
   | `.devcontainer/devcontainer.json` | none | none | `devcontainers` |

3. **Read the repository's details from GitHub**, where `gh` is signed in. Each of these calls only
   reads:

   ```bash
   gh repo view --json nameWithOwner,visibility,defaultBranchRef,hasDiscussionsEnabled
   gh api repos/OWNER/REPO/private-vulnerability-reporting
   gh api repos/OWNER/REPO/code-scanning/default-setup --jq .state
   gh label list --limit 200
   ```

   - The visibility decides what is available. Code scanning runs in a public repository, or in a
     private one with GitHub Code Security enabled. Private vulnerability reporting is for public
     repositories. Issue forms enforce required fields only in public repositories.
   - The default branch goes into the workflow's triggers.
   - The private vulnerability reporting call prints `{"enabled":true}` or `{"enabled":false}`.
   - The default setup call prints `configured` or `not-configured`. It needs more access than many
     tokens have, and then fails with `Resource not accessible by personal access token`. In that
     case, ask the user to look under **Settings** instead.
   - The labels are the ones the issue forms may use, because a form never creates a label.

   Without `gh`, ask the user for the visibility and the default branch.
4. **Find what already exists**, in every place GitHub looks:

   | Piece | Where GitHub looks |
   | --- | --- |
   | `CONTRIBUTING.md` and `CODEOWNERS` | `.github/`, the root, and `docs/`, and it uses the first one it finds, in that order |
   | `SECURITY.md` | the root, `.github/`, or `docs/` |
   | Pull request template | `pull_request_template.md` in `.github/`, the root, or `docs/`, or a `PULL_REQUEST_TEMPLATE/` folder in one of them |
   | Issue templates | `.github/ISSUE_TEMPLATE/`: forms (`.yml`), Markdown templates (`.md`), and `config.yml` |
   | Code scanning | any workflow that uses `github/codeql-action` |
   | Dependabot | `.github/dependabot.yml` |
   | Ignore rules | `.gitignore` in every folder, and `.git/info/exclude` |

   Keep one copy of each file, and ask before removing another. Also check whether the account or
   organization that owns the repository has a public repository named `.github`
   (`gh repo view OWNER/.github`). Its community files, such as `CONTRIBUTING.md`, `SECURITY.md`,
   and issue and pull request templates, apply to every repository that lacks its own. A file in
   this repository replaces the default. Any valid form, template, or `config.yml` here turns off
   all of the default issue templates. Ask whether the user wants to keep relying on the defaults.
5. **Show the survey** in a short table: each piece in scope, what exists, and what you plan.

## 3. Ask what only the user can decide

Ask in one message, about the pieces in scope only, with a proposed answer for each, so the user
can accept it in a word:

| Piece | Question | Proposed answer |
| --- | --- | --- |
| Security policy | How do reporters reach the project privately? | GitHub's private vulnerability reporting in a public repository; in a private one, an address the user gives |
| Security policy | Which versions get security fixes? | The latest release line, from the tags and the **Release** process |
| Security policy | What does the project promise: a time to acknowledge a report, a time to fix it, credit for the reporter? | Credit, and no deadlines unless the user sets them |
| Code owners | Who owns each area? Each owner is a GitHub user or team with write access | One team or person for everything, named by the user |
| Contributor guide | Under what terms are contributions accepted: the repository's license alone, a sign-off under the Developer Certificate of Origin (DCO), or a contributor license agreement (CLA)? | The repository's license alone |
| Contributor guide | Is there a code of conduct? | Link the one the repository has; write none unless asked |
| Contributor guide | Should contributors open an issue before a large change? | Yes, for anything beyond a small fix |
| Code scanning | For a private repository: is GitHub Code Security enabled? | Skip code scanning without it, because uploads would fail |
| Code scanning | Which query suite? | The default suite; `security-extended` where the **Untrusted input** names modules that read outside data |
| Code scanning | For Swift: run on macOS, whose runner minutes cost more in private repositories? | Yes, in a public repository; ask in a private one |
| Dependency updates | How often, and should updates share pull requests? | Weekly; one pull request for each ecosystem's minor and patch updates, and each major update on its own |
| Issue forms | Which forms, whether blank issues stay open, and where questions go | A bug report and a feature request, plus an erratum report for the **Errata** in teaching materials; blank issues off; questions to Discussions where they are on |
| Ignore rules | Where do editor and operating system files, such as `.DS_Store` and `.idea/`, go? | Each person's global ignore file, not the project's |

## 4. Ignore rules (`gitignore`)

### Collect the patterns

Take the patterns from these sources, in this order:

1. The **Never commit** paths.
2. What each adopted language rule says never to commit, such as keystores, signing files, and
   `local.properties`, and what it says to commit, such as lock files.
3. The ignore file each ecosystem's own tool writes for a new project, such as `cargo new` or
   `flutter create`. Where the tool is installed, create a throwaway project in a temporary folder
   outside the repository, and read the file it writes.
4. GitHub's templates in the `github/gitignore` repository, as a reference only. They are broad
   and can be wrong for a given project. The Dart template ignores `pubspec.lock`, which Dart's
   documentation says an app commits. The Kotlin template ignores every `*.jar`, which would
   ignore the Gradle wrapper unless an exception follows it.

| Rule | Ignore | Keep tracked |
| --- | --- | --- |
| `rust.md` | `/target/`, and the build and crash folders of fuzz targets, such as `fuzz/target/` and `fuzz/artifacts/` | `Cargo.lock`, which `rust.md` builds with `--locked`, and `rust-toolchain.toml` |
| `kotlin.md` | `.gradle/`, `build/`, and `.kotlin/`, where Kotlin 2.0 and later keep Gradle data | the Gradle wrapper (`gradlew`, `gradlew.bat`, and `gradle/wrapper/`), `gradle/libs.versions.toml`, and any `gradle.lockfile` or `gradle/verification-metadata.xml` |
| `android.md` | `local.properties`, keystores (`*.jks` and `*.keystore`), built packages (`*.apk` and `*.aab`), and `.cxx/` | Room's exported schema files, which `android.md` says to commit, and the R8 and ProGuard rules |
| `kotlin-multiplatform.md` | the `kotlin.md` and `swift.md` rows, for the shared modules and the iOS app | the iOS app's Xcode project |
| `swift.md` | `xcuserdata/`, `.build/`, `DerivedData/` where the project builds inside the repository, and signing files (`*.p12` and `*.mobileprovision`) | `Package.resolved`, the `.xcodeproj` and `.xcworkspace` apart from their `xcuserdata/`, and shared schemes in `xcshareddata/` |
| `flutter.md` | `.dart_tool/`, `build/`, `.flutter-plugins-dependencies`, `android/key.properties`, and keystores | `pubspec.lock` in an app, though not in a package that others depend on, as Dart's documentation says; and the generated files that **Code generation** says are committed |
| `lua.md` | LuaCov's `luacov.stats.out` and `luacov.report.out`, a local rocks tree such as `lua_modules/`, packed rocks (`*.rock`), and compiled C modules (`*.o`, `*.so`, and `*.dll`) | the rockspecs, and `luarocks.lock` where the **Dependency policy** pins versions with it |
| `web-front-end.md` | `node_modules/`, and the built site, such as `_site/`, `.jekyll-cache/`, or `dist/`, as **Build and dependencies** says | the lock files: `package-lock.json`, `pnpm-lock.yaml`, `yarn.lock`, `bun.lock`, and `Gemfile.lock` |
| `teaching-materials.md` | LaTeX's build files, `.ipynb_checkpoints/`, `__pycache__/`, `.venv/`, the built book (`_build/`), and nbgrader's `gradebook.db`, which holds student records | the sources, and PDFs only where the rule says to commit them |
| Every repository | `.env` and `.env.*` with the exception `!.env.example`, and Claude Code's personal files, `.claude/settings.local.json` and `CLAUDE.local.md` | `.claude/settings.json`, `.claude/rules/`, `.claude/skills/`, `CLAUDE.md`, `AGENTS.md`, and `.github/` |

Claude Code adds `.claude/settings.local.json` to the user's global ignore file when it creates the
file, but not when a person creates it by hand, so the project's file lists it too.

### Write the file

- Group the patterns by source, under a comment that says what each group keeps out and why, such
  as `# nbgrader's gradebook, which holds student records`.
- Anchor a path that exists only at the root with a leading `/` (`/target/`). Without a `/` at the
  start or in the middle, a pattern matches at every level. End a pattern meant only for folders
  with `/`.
- Put each exception (`!pattern`) after the pattern it overrides, because the last matching
  pattern wins. An exception can't bring back a file inside an ignored folder, because Git doesn't
  look inside a folder it ignores. So ignore `.claude/settings.local.json`, never `.claude/` with
  exceptions after it.
- Check each broad pattern, such as `*.json`, `*.js`, `*.zip`, `*.out`, or `build`, against
  `git ls-files`, because it can match source files and test fixtures.
- Leave personal files out. Git's documentation puts files that only one person's tools create,
  such as editor settings, in that person's global ignore file (`core.excludesFile`, by default
  `~/.config/git/ignore`), and files that only one clone needs in `.git/info/exclude`. Add them to
  the project's file only if the user asks.
- Keep the existing file's lines and their order, and add new groups after them. Ask before you
  remove or reorder a line.

### Handle files that are already tracked

Ignoring a file doesn't remove it from Git. List the tracked files that the ignore rules match:

```bash
git ls-files -ci --exclude-standard
```

It prints one path per line, and nothing when no tracked file is ignored. For each path:

- **Build output or a local setting:** propose `git rm --cached <path>`, which removes the file from
  the next commit and leaves it on disk. Ask first: once that commit is pulled, Git deletes the file
  from everyone else's working tree.
- **A secret:** stop. It is in the history, and every clone and fork has a copy. Tell the user to
  revoke it at its provider and issue a new one, and suggest `/security-sweep` where the project
  has it. Rewriting the history is the user's decision.
- **Answer keys or student records** that the **Assessment** keeps private: tell the user. An
  ignore rule doesn't unpublish what is already pushed.

## 5. Code scanning (`codeql`)

### Choose advanced setup

GitHub runs CodeQL in one of two ways:

- **Default setup** is a setting: GitHub picks the languages and runs the analysis, with no file in
  the repository.
- **Advanced setup** is a workflow file: its languages, build steps, query suites, and pinned
  actions are reviewed and changed like code.

This skill writes advanced setup. The two don't combine: while default setup is on, GitHub disables
the workflow and blocks its uploads. If step 2 found default setup `configured`, ask whether to keep
it and skip this piece, or switch, which the user does by turning default setup off (step 11). If
a workflow already uses `github/codeql-action`, update that one instead of adding a second.

### Pick the languages and build modes

Analyze each CodeQL language from step 2 that has source files in the repository, and add
`actions`, which checks the workflows, this one included. Give each language a build mode:

| Build mode | Languages | What it does |
| --- | --- | --- |
| `none` | `actions`, `javascript-typescript`, `python`, `ruby`, `rust`, `csharp`, and `java-kotlin` when the code is only Java | Analyzes the source without building it |
| `autobuild` | `java-kotlin` with any Kotlin, `swift`, `go`, and `c-cpp` | Tries to build the code the usual way for its language |
| `manual` | any compiled language | Runs the build steps the workflow gives, from the **Check commands** |

- Kotlin needs a build. With `none`, CodeQL leaves out the Kotlin code and only warns.
- Swift is analyzed only on macOS runners.
- CodeQL analyzes only what a `manual` or `autobuild` build compiles, so the build must cover every
  module and target. Use `manual` when `autobuild` can't build the project on the runner.
- Name languages with the identifiers in this table. Older workflows use aliases such as `java` or
  `javascript`.

### Look up the actions' releases

For each action, find the newest release tag, then the commit it points to. On November 23, 2026,
for `github/codeql-action`:

```bash
git ls-remote --tags --refs --sort=-version:refname \
  https://github.com/github/codeql-action 'v4.*' | head -n 1
gh api repos/github/codeql-action/commits/v4.39.4 --jq .sha
```

```text
e0462179768953fd6790193e15caaef674ff709e	refs/tags/v4.39.4
1bd86f2fdb4b86187fa4059f16f31a3202acc9ea
```

- Pin the hash the second command prints. The two differ because `v4.39.4` is an annotated tag:
  the first hash is the tag's own object, and the second is its commit. Without `gh`, run
  `git ls-remote <url> 'refs/tags/<tag>^{}'`, which prints the commit an annotated tag points to.
- For `github/codeql-action`, take the newest tag of the newest major version, not its newest
  release: in November 2026, the release marked **Latest** was a CodeQL bundle
  (`codeql-bundle-v2.27.5`), which isn't the action. GitHub has announced that v3 of the action is
  deprecated from December 2026.
- `init` and `analyze` come from the same repository, so they share one SHA.

### Write the workflow

Start from this shape, with the languages from the table and the SHAs and tags you looked up:

```yaml
name: "CodeQL"

on:
  push:
    branches: ["main"]
  pull_request:
    branches: ["main"]
  schedule:
    - cron: "27 4 * * 1"

permissions: {}

jobs:
  analyze:
    name: "Analyze (${{ matrix.language }})"
    runs-on: ${{ (matrix.language == 'swift' && 'macos-latest') || 'ubuntu-latest' }}
    permissions:
      security-events: write # upload the results
      contents: read # check out the code
      actions: read # read the workflow's run, in private repositories
    strategy:
      fail-fast: false
      matrix:
        include:
          - language: "actions"
            build-mode: "none"
          - language: "rust"
            build-mode: "none"
    steps:
      - name: "Check out the repository"
        uses: actions/checkout@<commit SHA> # <tag>
        with:
          persist-credentials: false
      - name: "Initialize CodeQL"
        uses: github/codeql-action/init@<commit SHA> # <tag>
        with:
          languages: ${{ matrix.language }}
          build-mode: ${{ matrix.build-mode }}
      - name: "Analyze"
        uses: github/codeql-action/analyze@<commit SHA> # <tag>
        with:
          category: "/language:${{ matrix.language }}"
```

- Put the default branch from step 2 in `branches`, with any protected release branches.
- Choose a `cron` minute and hour away from the start of the hour. GitHub can delay scheduled runs
  when load is high, which includes the start of every hour. The time is in Coordinated Universal
  Time (UTC).
- `permissions: {}` at the top level gives the token no access by default, and the job grants only
  what it needs.
- `persist-credentials: false` stops checkout from leaving the token in `.git/config`, where later
  steps could read it.
- For a `manual` language, add its build steps between `init` and `analyze`, under
  `if: matrix.build-mode == 'manual'`, and set up the toolchain the **Toolchain** names.
- For a query suite, add `queries: security-extended` to `init`'s `with`.

## 6. Dependency updates (`dependabot`)

`.github/dependabot.yml` sets up version updates: pull requests that move each dependency to its
newest release. Security updates, which fix known vulnerabilities, are a setting (step 11), though
a group with `applies-to: security-updates` in this file decides how they are grouped. Start from
this shape:

```yaml
version: 2
updates:
  - package-ecosystem: "cargo"
    directory: "/"
    schedule:
      interval: "weekly"
    cooldown:
      default-days: 7
    groups:
      cargo-minor-and-patch:
        patterns: ["*"]
        update-types: ["minor", "patch"]
    commit-message:
      prefix: "build"
      include: "scope"

  - package-ecosystem: "github-actions"
    directory: "/"
    schedule:
      interval: "weekly"
    cooldown:
      default-days: 7
    groups:
      actions-minor-and-patch:
        patterns: ["*"]
        update-types: ["minor", "patch"]
    commit-message:
      prefix: "ci"
      include: "scope"
```

- Add one entry for each ecosystem from step 2, with the folder that holds its manifest. For one
  ecosystem in several folders, list them under `directories` instead.
- Add the `github-actions` entry whenever the repository has workflows. `directory: "/"` finds the
  files in `.github/workflows/`. Dependabot raises the pinned SHAs; when you review its pull
  requests, check that each tag comment still names the release the SHA belongs to.
- `cooldown` makes Dependabot wait that many days after a release before it proposes it, so a
  release that turns out broken or malicious has time to be found and pulled first. It doesn't
  delay security updates.
- Each group puts an ecosystem's minor and patch updates in one pull request. Major updates stay
  out of it, so each arrives on its own, because a new major version may change the API.
- The prefixes follow the types in `commits.md`: `build` for dependency manifests and lock files,
  and `ci` for workflows. `include: "scope"` adds `(deps)` or `(deps-dev)` after the prefix. If
  `deps` isn't one of the **Scopes**, ask whether to add it or leave out `include`.
- Follow the **Dependency policy**. Leave out an ecosystem whose policy is "none". Where the policy
  holds a dependency at a version on purpose, add an `ignore` entry with a comment that names the
  reason, and ask first.
- Leave `open-pull-requests-limit` at its default unless the user asks.
- Dependabot has nothing to read for an ecosystem without support, such as LuaRocks. Say in the
  report that those dependencies need checking by hand.
- Workflows that run on Dependabot's pull requests get a read-only token and only Dependabot
  secrets, not Actions secrets. A check that needs a secret fails there; tell the user, and never
  work around it with `pull_request_target`.

## 7. Security policy (`security`)

Write `SECURITY.md` in the root, or where step 2 found it. Use what the user decided in step 3, and
leave out a section the user hasn't decided:

1. **Supported versions:** a table of the release lines, and whether each gets security fixes.
2. **Report a vulnerability:**
   - With private vulnerability reporting: the **Security and quality** tab, then
     **Report a vulnerability**, which opens the form at
     `https://github.com/OWNER/REPO/security/advisories/new`.
   - Otherwise, the private contact the user gave.
   - Say plainly that reports never go in a public issue, discussion, or pull request.
3. **What to include:** the affected version, the steps or proof of concept that show the problem,
   its impact, and how to reach the reporter. Ask reporters to test only on their own systems and
   data, never against someone else's deployment.
4. **What happens next:** only what the user promised: when the project acknowledges a report,
   keeps the reporter informed, and credits them. Where the user agrees, say that each fix is
   announced in a GitHub security advisory, which GitHub can give a Common Vulnerabilities and
   Exposures (CVE) ID.
5. **Scope:** what the policy covers, such as this repository's code and releases, and what it
   doesn't, such as a vulnerability in a dependency, which goes to that project's own policy.

Promise no reward and no legal protection for researchers unless the user supplies that text.

## 8. Contributor guide (`contributing`)

Write `CONTRIBUTING.md` in the root, or where step 2 found it, from the adopted rules. Link a rule
rather than copy it, so the guide doesn't drift from it:

1. **Welcome:** what kinds of contributions the project takes, and, if the user said so, to open an
   issue before a large change.
2. **Set up:** the toolchain from the **Toolchain** or **Lua version**, and the commands that
   install the dependencies.
3. **Make a change:** where code and tests go (**Test layout**), and the **Check commands** in a
   code block, exactly as the rule gives them, with what a passing run looks like.
4. **Commit:** Conventional Commits with the **Scopes** and **Task IDs**, one change per commit,
   with a link to `.claude/rules/commits.md`, and a link to `.claude/rules/writing.md` for the
   text. Where the user chose a DCO sign-off, say that `git commit -s` adds the `Signed-off-by:`
   line it requires.
5. **Documentation:** which documents each kind of change updates, from the **Update map**. Where
   the **Update map** names a section of `CONTRIBUTING.md`, this is that section, so keep it in
   step with the map.
6. **Pull requests:** the template, the checks that must pass, and who reviews (step 10).
7. **Report a problem:** the issue forms for bugs and requests, and `SECURITY.md` for
   vulnerabilities.
8. **Terms:** the terms from step 3, and the code of conduct, if there is one.
9. **Coding agents:** where agents find their instructions, from the **Agent guide**, or
   `AGENTS.md` and `.claude/rules/`.

For teaching materials, add how corrections reach the **Errata**, and ask contributors never to
post a solution in an issue or a pull request, as the **Assessment** requires.

## 9. Issue forms and the pull request template (`issues`, `pr`)

### Issue forms

Each form is a YAML file in `.github/ISSUE_TEMPLATE/`, such as `bug-report.yml`. It starts with
`name`, `description`, and `body`. The `name` must be unique among all the templates and longer
than three characters, or GitHub doesn't show the form. For example:

```yaml
name: "Bug report"
description: "Report something that doesn't work as documented."
labels: ["bug"]
body:
  - type: markdown
    attributes:
      value: "To report a security vulnerability, don't use this form: follow the
        [security policy](https://github.com/OWNER/REPO/security/policy). Leave passwords,
        tokens, and personal data out of this report."
  - type: input
    id: version
    attributes:
      label: "Version"
      description: "The release or commit you use, such as v1.2.0."
    validations:
      required: true
  - type: textarea
    id: steps
    attributes:
      label: "Steps to reproduce"
    validations:
      required: true
  - type: textarea
    id: result
    attributes:
      label: "What you expected, and what happened instead"
    validations:
      required: true
  - type: textarea
    id: logs
    attributes:
      label: "Error messages or logs"
      render: "text"
```

- Build the fields from the project: a `dropdown` of the **Platforms**, **Targets**, or
  **Browser support**, and the toolchain version for a library.
- `labels` only applies labels that exist. List each missing one for the user to create (step 11).
- `validations.required` stops a submission only in public repositories.
- Add a `type` only if the user names an issue type their organization defines.
- A number at the start of each file name, such as `01-bug-report.yml`, orders the forms in the
  chooser.
- An erratum report for teaching materials asks for the material, the place in it, what is wrong,
  and the source that shows it, and asks students not to post solutions.
- Keep the repository's existing Markdown templates, and offer to turn them into forms.

Then write `.github/ISSUE_TEMPLATE/config.yml`, which shapes the chooser:

```yaml
blank_issues_enabled: false
contact_links:
  - name: "Report a security vulnerability"
    url: "https://github.com/OWNER/REPO/security/advisories/new"
    about: "Report it privately, as the security policy describes."
  - name: "Ask a question"
    url: "https://github.com/OWNER/REPO/discussions"
    about: "Ask and answer questions in Discussions."
```

With `blank_issues_enabled: false`, people with write access still see a **Blank issue** choice,
marked for maintainers only. Link Discussions only where step 2 found them on, and link the
advisory form only where private vulnerability reporting is, or will be, on.

### Pull request template

Write `.github/pull_request_template.md`, with the sections `commits.md` asks a pull request's
description to cover:

```markdown
## What changed and why

<!-- The change, and the problem it solves. Link the issue it closes: "Closes #12". -->

## How it was checked

<!-- Each check you ran, and its result. -->

- [ ] The check commands pass
- [ ] New behavior has tests

## Left for later

<!-- Breaking changes, follow-up work, and anything a reviewer could miss. -->
```

- Replace the checklist with the project's own: each **Check commands** entry by name, the
  **Changes that travel together**, and a line for **Breaking changes**. Keep it short, because
  reviewers and authors skip long checklists.
- The comments (`<!-- … -->`) guide the author and don't show in the rendered description.
- Keep one template. With a `PULL_REQUEST_TEMPLATE/` folder of several, contributors must choose
  one through a `template` query parameter in the URL.

Issue forms and the pull request template take effect once they are on the default branch.

## 10. Code owners (`codeowners`)

Write `.github/CODEOWNERS` with the owners the user named in step 3. GitHub reads `.github/`, then
the root, then `docs/`, and uses the first file it finds, so merge any other copy into this one,
and ask before removing it.

```text
# The last matching pattern wins, so the catch-all comes first.
*                    @example-org/developers

# Changes to CI, to ownership, and to the security policy need a maintainer.
/.github/            @example-org/maintainers
/SECURITY.md         @example-org/maintainers

/docs/               @example-org/writers @example-user
```

- An owner is `@user`, `@org/team`, or an email address on a user's account. Each must have
  explicit write access to the repository, and a team must be visible. GitHub assigns no review
  to an owner without access.
- Put all of a pattern's owners on one line. A later line with the same pattern replaces the
  earlier one.
- A pattern followed by no owners leaves the files it matches without owners.
- Patterns follow `.gitignore` syntax, except that `!`, character ranges in `[ ]`, and `\#` don't
  work. Paths are case sensitive.
- GitHub skips a line with invalid syntax and applies the rest, and doesn't load a file of 3 MB or
  more.
- Review requests come from the `CODEOWNERS` file on the pull request's base branch.
- A code owner's review is only requested, not required, until the user turns on
  **Require review from Code Owners** in a branch protection rule or a ruleset (step 11).

Check that each pattern matches tracked files. Git's own matcher reads the syntax that
`CODEOWNERS` supports:

```bash
git ls-files -ci --exclude='/docs/'
```

It prints each tracked file the pattern matches, and nothing when it matches none. Fix or remove a
pattern that matches nothing.

## 11. List the settings for the user

Show each setting the pieces need, with where GitHub keeps it. In November 2026, the security
settings are on the **Advanced Security** page, in the **Security and quality** section of the
repository's **Settings**:

| Setting | Where | Why |
| --- | --- | --- |
| Private vulnerability reporting | **Advanced Security** | The security policy's link opens the report form only when it is on |
| Dependency graph, Dependabot alerts, and Dependabot security updates | **Advanced Security** | Security updates need the other two |
| Code scanning's default setup, turned off | **Advanced Security** | Only when switching to the workflow (step 5) |
| Secret scanning and push protection | **Advanced Security** | Push protection blocks a push that contains a secret |
| GitHub Code Security | The organization's plan, then **Advanced Security** | Code scanning in a private repository |
| Each label the forms use | **Issues**, then **Labels** | A form never creates one |
| **Require review from Code Owners**, and the required checks | A branch protection rule or a ruleset for the default branch | Without it, code owners are only asked |
| **Workflow permissions** set to read-only | **Settings**, **Actions**, **General** | Workflows without a `permissions` key then get a read-only token |

## 12. Verify

1. **Every YAML file parses:** run the check in "Before finishing" in `writing.md` on the
   workflow, `dependabot.yml`, each form, and `config.yml`.
2. **The workflow is sound:** run actionlint and `zizmor --offline .github/workflows/`, where they
   are installed, and ask before installing them. Check that every `uses:` line names a
   40-character SHA and a tag comment.
3. **The ignore rules do what they should.** Check paths that must be ignored and paths that must
   stay tracked, then the tracked files the rules match:

   ```bash
   git check-ignore -v --no-index target/release/app .env
   git check-ignore -v --no-index Cargo.lock gradle/wrapper/gradle-wrapper.jar
   git ls-files -ci --exclude-standard
   ```

   For each path that a pattern matches, `-v` prints the file, line, and pattern that decide it.
   For a `.gitignore` whose first four lines are `/target/`, `.env`, `*.jar`, and
   `!gradle/wrapper/gradle-wrapper.jar`, the first two commands print:

   ```text
   .gitignore:1:/target/	target/release/app
   .gitignore:2:.env	.env
   .gitignore:4:!gradle/wrapper/gradle-wrapper.jar	gradle/wrapper/gradle-wrapper.jar
   ```

   A pattern that starts with `!` keeps its path. A path that must stay tracked should print
   nothing, or only such a line. The paths don't need to exist.
4. **Each `CODEOWNERS` pattern matches files** (step 10).
5. **The documents are right:** run every command `CONTRIBUTING.md` shows, and confirm that each
   link and path in the new files resolves. Preview the Markdown with the **Preview command**, and
   do the checks in "Before finishing" in `writing.md`.
6. **The project's checks pass:** run the **Done checks**.
7. **On GitHub, once the user pushes:** list these for the user, or run the read-only ones:
   - the **CodeQL** workflow's run in the **Actions** tab passes for each language;
   - Dependabot's first update run for each ecosystem ends without an error about
     `dependabot.yml`;
   - `gh api "repos/OWNER/REPO/codeowners/errors?ref=BRANCH"` prints an empty `errors` list, where
     a line with an unknown owner or bad syntax would appear;
   - the **New issue** chooser shows the forms and the contact links.

## 13. Commit only as the project allows

Follow the **Git access** level in `commits.md`, and leave the changes uncommitted unless the user
asked for a commit and that level allows it. Either way, end the report with copy-ready commit
messages, as `commits.md` describes, one for each piece, in this order:

1. the ignore rules, with any files untracked by `git rm --cached`: `chore`;
2. `SECURITY.md`: `docs`;
3. `CONTRIBUTING.md`: `docs`;
4. the issue forms and the pull request template: `chore`;
5. `CODEOWNERS`: `chore`;
6. `dependabot.yml`: `chore`;
7. the CodeQL workflow: `ci`.

Use a scope only where one of the **Scopes** fits. Before a push the user asked for, say what the
**Push effect** sets off, and add what these files set off: on the default branch, the CodeQL
workflow runs at once and then every week, and Dependabot starts opening pull requests.

## 14. Report

End with a summary that contains:

- **Scope:** the pieces set up, and those left out and why.
- **Survey:** the languages and ecosystems found, the repository's visibility, and the files that
  already existed.
- **Files:** each file written or changed, and what it does.
- **Decisions:** each answer from step 3, and each question the user left open.
- **For you to do:** each setting from step 11, with its steps, and each label to create.
- **Checks:** each check from step 12, with its real result, and each one you couldn't run, and
  why.
- **Not covered:** dependencies Dependabot can't update, languages CodeQL can't analyze, and
  anything else waiting for the user.
- **Commit messages:** in `text` blocks, as `commits.md` describes, with no attribution lines.
