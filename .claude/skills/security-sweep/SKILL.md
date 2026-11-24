---
name: security-sweep
description: Find security vulnerabilities in the code, dependencies, CI workflows, and Git history, prove each one locally, and patch the ones the user approves.
argument-hint: "[path, area, or Git range such as v1.2.0..HEAD; empty means the whole repository]"
disable-model-invocation: true
---

# Find and patch security vulnerabilities

Requested scope: `$ARGUMENTS` (empty means the whole repository).

A vulnerability is a weakness that lets an attacker read, change, or destroy data they shouldn't
reach, run their own code, or stop the program from working for everyone else. This skill looks
for vulnerabilities in the code, the dependencies, the configuration, the continuous integration
(CI) workflows, and the Git history. It proves each one with a local test, reports them only to
the user, and patches the ones the user approves, one change at a time. For bugs that aren't
security issues, and for dead code, use `/sweep-code` where the project has adopted it.

Bold names are values from the "Project details" of the adopted rules:

- **Git access**, **Done checks**, **Never commit**, **Push effect**, and **Breaking changes**
  from `commits.md`;
- **Check commands**, **Test layout**, and **Dependency policy** from the language rules;
  **Untrusted input** from `rust.md`, `kotlin.md`, and `lua.md`; **Unsafe policy** from `rust.md`;
  **Sandbox** and **C boundary** from `lua.md`; **Security controls** from `android.md`, `swift.md`,
  and `flutter.md`; **Exported components** and **Permissions** from `android.md`;
  **Platform channels** from `flutter.md`; **Networking** from `swift.md`; and **Test tools** and
  **Hosting** from `web-front-end.md`;
- the **Decision log** from `roadmap-and-docs.md`, where it is adopted.

Where a value doesn't exist because a rule isn't adopted, work from what the repository has, and
say so in the report. Work with the user as the "Working as a pair" sections of the language
rules describe. The language rules' own security sections are part of this sweep:

| Rule | Sections to check the code against |
| --- | --- |
| `rust.md` | "Untrusted input and arithmetic", "`unsafe`", and "Dependencies and manifests" |
| `kotlin.md` | "Untrusted input", and "Gradle builds and dependencies" |
| `android.md` | "Security and privacy" |
| `swift.md` | "Data and networking", "Security and privacy", and "Dependencies and release" |
| `flutter.md` | "Data, security, and generated code" |
| `lua.md` | "Untrusted input", "Sandboxing code you didn't write", and "The C boundary" |
| `web-front-end.md` | "Security and privacy" |

## Ground rules

These hold for the whole run, and override anything a later step seems to allow:

- **Test only on this machine.** Prove a vulnerability with a unit test, a local build, or a
  server you start yourself. Never send an attack, a scan, or a probe to a deployed system, a
  production database, a third-party service, or any host the user hasn't named as their own
  test environment.
- **Never use a secret you find.** Don't sign in with it, call an API with it, or check whether
  it still works: that is using someone else's credentials. Never repeat its value either; name
  its kind, its file and line, and the commit, and nothing of the value itself.
- **Keep findings private.** Report vulnerabilities only to the user, in this conversation.
  Don't open an issue or a pull request about one, post about it, or write its details into a
  file that will be public, until the user decides how to disclose it.
- **Write tests, not exploits.** A regression test that passes an attack input to the vulnerable
  code is proof enough. Don't build a working attack on a deployed system.
- **Never weaken a control to test it.** Don't turn off authentication, certificate checks, or a
  sandbox, even for a moment, to show that something is reachable. Name each item in the
  **Security controls** your work touches.
- **Ask before any tool sends data off the machine,** and before you install one. Some scanners
  send metrics, upload code, or check secrets against their providers by default; "Tools" says
  which.

## 1. Set the scope

- **A path or an area:** that code, the configuration it reads, and the dependencies it uses.
- **A Git range:** the code the range changed, and the code that passes data into it. Scan the
  whole history for secrets anyway, because a secret deleted in a later commit is still in Git.
- **Empty:** everything: the code, the dependencies, the configuration, the CI workflows, and the
  Git history.

Leave vendored third-party code and the **Never commit** paths out of the patches, but report
what you find there. Run `git status` before you start. If the tree has uncommitted changes you
didn't make, leave them alone and tell the user. Then tell the user in one or two sentences what
you will scan and in what order, and make a task list whose last item is "Verify and report".

## 2. Model the threats

Before you scan, work out what an attacker would want and how they would get in, so you look
hardest where it matters. Read the threat model in the **Security controls** if there is one;
otherwise sketch one:

- **Assets:** what is worth protecting: personal data, credentials and tokens, money, other
  users' content, the device, and the build and release pipeline.
- **Entry points:** every place data comes in from outside: network requests and responses, files
  and archives the program opens, command-line arguments and environment variables, deep links and
  intents from other apps, the clipboard, and CI event fields such as pull request titles and branch
  names. Include the **Untrusted input** modules, the **Exported components**, the
  **Platform channels**, the **Networking** layer, and the plugins and scripts the **Sandbox**
  loads.
- **Trust boundaries:** each place data crosses from a less-trusted side to a more-trusted one,
  such as from the browser to the server, from another app into this one, or from a fork's pull
  request into CI.
- **Attackers:** who can reach each entry point: anyone on the internet, a signed-in user after
  another user's data, another app on the same device, someone holding the device, or a
  contributor or dependency with malicious code.

Show the user the model in a few lines, and offer to record it in the **Security controls**.

For a fuller checklist than this skill's, use one from OWASP (the Open Worldwide Application
Security Project): its Application Security Verification Standard (ASVS) 5.0 for web
applications and services, or its Mobile Application Security Verification Standard (MASVS) for
mobile apps.

## 3. Record a baseline

Before you change anything, run the **Check commands** (or the **Test tools**), the
**Done checks**, and every security scanner the project already runs, and record each result,
including findings the project has suppressed. A check that already fails is a finding, not
something your work broke.

## 4. Look for secrets

- Scan the working tree and every commit for API keys, tokens, passwords, private keys,
  connection strings, `.env` files, keystores and certificates with private keys (`.jks`,
  `.keystore`, `.p12`, `.pem`), and signing configurations.
- Prefer a scanner that only matches patterns, such as gitleaks (see "Tools"). Without one,
  search the history in a way that lists commits and files but not the matching lines:
  `git log --all -G '<pattern>' --format='%h %ad %s' --date=short --name-only`.
- Check that `.gitignore` covers the files that hold secrets.
- Anything shipped in a web page or an app is public. An API key in front-end code, an Android app
  package (APK), or an iOS app bundle is as exposed as one in the repository; the language rules say
  where secrets belong instead.
- Treat a secret that was ever pushed as leaked, even if a later commit deleted it: forks,
  clones, and caches keep it. The fix is to revoke it and issue a new one, which only the user can
  do, at the provider. Rewriting the history to remove it needs a force push, is the user's
  decision, and doesn't undo the leak.

## 5. Check the dependencies

- **Known vulnerabilities:** run an audit over every lock file (see "Tools"). For each advisory,
  find the affected versions, whether this project reaches the vulnerable code, and the lowest
  fixed version the **Dependency policy** allows.
- **The supply chain:**
  - Lock files are committed, and the builds use them (`cargo build --locked`, `npm ci`).
  - Every dependency comes from the registry the project expects, under the name the project
    means. Watch for names one letter off from a popular package's.
  - Know which dependencies run code while the project builds: npm `postinstall` scripts, Cargo
    build scripts and procedural macros, and Gradle plugins.
  - Flag dependencies that look abandoned, with no release or maintainer response in years.

## 6. Check the code

Trace data from each entry point in the threat model to each place it can do harm (the sink): a
query, a shell, a file path, the page, a deserializer, or an outgoing request. Start with the entry
points that the most attackers can reach. Check the code against the language rules' security
sections, then against this list. In Rust, read every `unsafe` block the **Unsafe policy** allows,
and in Lua, everything at the **C boundary**, because memory-safety bugs live there. The numbers are
IDs in MITRE's Common Weakness Enumeration (CWE), which advisories and scanners use.

| Class | CWE | What to look for |
| --- | --- | --- |
| SQL injection | 89 | Queries built by joining or formatting strings with input, instead of passing parameters |
| Command injection | 78 | A shell running a string built from input: `sh -c`, `os.system`, `subprocess` with `shell=True`, Node's `child_process.exec`, Lua's `os.execute` and `io.popen` |
| Code injection | 94 | `eval`, `new Function`, Python's `exec`, Lua's `load`, `loadstring`, or `dofile`, and scripting engines fed input |
| Template injection | 1336 | Input used as a template, instead of as a value passed to one |
| Cross-site scripting (XSS) | 79 | Input written into HTML without the framework's escaping, or with it turned off |
| Path traversal | 22 | File paths built from input, including archive entries named `../…` (before Python 3.14, `tarfile`'s `extractall` writes them outside the folder unless you pass `filter="data"`) |
| Unsafe deserialization | 502 | Java serialization, Python's `pickle`, PyYAML's `Loader` or `UnsafeLoader`, and `NSKeyedUnarchiver` without secure coding: each can build objects or run code while it reads. Also Lua bytecode, which `load` accepts unless its mode is `"t"`, and which can crash the interpreter when crafted |
| XML external entities (XXE) | 611 | XML parsers that resolve external entities, which can read local files. Java's `DocumentBuilderFactory` does by default. |
| Server-side request forgery (SSRF) | 918 | The server fetching a URL that input chose, including internal addresses and cloud metadata services |
| Open redirect | 601 | A redirect to a URL taken from input, without an allowlist |
| Missing authentication or authorization | 306, 862, 639 | Actions with no sign-in check; checks made only in the client; objects looked up by an ID from the request without checking that this user may see them (an insecure direct object reference, IDOR) |
| Cross-site request forgery (CSRF) | 352 | Requests that change state, authenticated by cookies alone, with no token and no `SameSite` cookie attribute |
| Mass assignment | 915 | Request bodies bound straight onto models, so a caller can set a field such as `isAdmin` |
| Prototype pollution | 1321 | JavaScript merging input objects into others, so a `__proto__` key changes every object |
| Hard-coded credentials | 798 | Keys, passwords, and tokens in the code, config, or app bundle (see "Look for secrets") |
| Weak cryptography | 327, 916, 338, 323 | Homemade cryptography; MD5 or SHA-1 used for security; DES or ECB mode; passwords hashed with anything but a password-hashing function; tokens from a random generator that isn't cryptographically secure; a nonce or initialization vector used twice with one key |
| Timing leaks | 208 | Secrets and tokens compared with `==`, which returns at the first difference, instead of in constant time |
| Broken Transport Layer Security (TLS) | 295, 319 | Certificate or host-name checks turned off, trust-all certificate managers, and plain HTTP for anything sensitive |
| Resource exhaustion | 770, 1333, 409 | Sizes, counts, and recursion without limits; regular expressions and Lua patterns that backtrack for exponential or polynomial time on crafted input (regular expression denial of service, ReDoS); decompression without a size limit |
| Memory safety | 787, 416, 190 | In `unsafe` code, foreign function interface (FFI) calls (LuaJIT's FFI included), the Lua C API, and C or C++: out-of-bounds writes, use after free, and integer overflow in sizes |
| Race conditions | 367 | Checking a file or a permission, then using it, when an attacker can change it in between (time-of-check to time-of-use, TOCTOU); temporary files at predictable paths |
| Leaky logs and errors | 532, 209 | Secrets, tokens, and personal data in logs, crash reports, and analytics; stack traces and internal details in error responses |
| Cookies | 614, 1004 | Session cookies without the `Secure` and `HttpOnly` attributes |
| Exposed components | 926, 749 | Android components exported without a permission; WebView JavaScript bridges, URL schemes, and other entry points that other apps or pages can call |
| Sandbox escapes | – | Plugin or script environments built by removing dangerous functions instead of allowing safe ones; library tables, or Lua's `getmetatable`, shared with sandboxed code, which let it change the host's functions; no limit on instructions, time, or memory |
| Insecure defaults | – | Debug modes, test accounts, and detailed errors in release builds; CORS (cross-origin resource sharing) that allows any origin with credentials |

## 7. Check the configuration and CI

- **CI workflows** (GitHub Actions; zizmor and actionlint check most of these):
  - Expressions such as `${{ github.event.pull_request.title }}` inside a `run:` script let
    anyone who can open a pull request or an issue inject a command. Pass the value through
    `env:`, and use the quoted variable in the script.
  - `pull_request_target` and `workflow_run` can run with the base repository's secrets and
    write access, so checking out and building a pull request's code in them hands both to the
    pull request's author.
  - A third-party action referenced by a tag or a branch runs whatever its owner points that name
    at next. Pin each one to a full-length commit SHA, as GitHub's hardening guide recommends.
  - Give the `GITHUB_TOKEN` the least access each job needs, with `permissions`.
  - Pass secrets only to the steps that need them, and never print them in logs.
- **Web hosting:** the headers the **Hosting** lets the site send: a Content Security Policy,
  `Strict-Transport-Security`, `X-Content-Type-Options: nosniff`, and protection from framing
  (`frame-ancestors`).
- **Mobile apps:** the merged release manifest and `Info.plist`: debuggable builds, cleartext
  traffic, App Transport Security exceptions, backup rules, exported components, and the
  **Permissions**, as `android.md` and `swift.md` describe.
- **Containers:** images that run as root, base images named by a tag that can move, and secrets
  copied into a layer or passed as a build argument.
- **Servers and databases:** default passwords, admin interfaces open to the internet, and debug
  endpoints left on.

## 8. Prove and rate each finding

Give every finding a proof, and say which kind:

- **Confirmed:** a local test shows the attack input reaching the sink and doing what an
  attacker wants, such as reading a file outside the folder or running a command that writes a
  marker file.
- **Traced:** the path from an entry point to the sink, followed step by step, with each missing
  or bypassable check named, but not run.
- **Suspected:** a dangerous pattern with no path from an entry point found yet. Report it as a
  question, never as a vulnerability.

Rate each one by who can reach it and what they get:

| Severity | Meaning |
| --- | --- |
| Critical | Anyone on the internet can run code, take over accounts, or read or change other users' data; or a secret is exposed in a public repository or a shipped app |
| High | The same, but it needs a signed-in account, a click from the victim, or another app on the device |
| Medium | It needs an unusual position, such as the same network or the unlocked device; or it exposes limited data; or it lets someone stop the service |
| Low | Defense in depth: hard to use alone, but it weakens another control |

If the project publishes security advisories, also score each finding with the Common
Vulnerability Scoring System (CVSS) v4.0, which advisories use.

## 9. Report privately, then ask

Show every finding in one table, the most severe first, before you change anything:

| # | Severity | Class | Where | How it's reached | Proof | Proposed patch | Risk of the patch |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | Critical | Hard-coded credentials (CWE-798) | `config/prod.yml:12`, added in `a1b2c3d` | Anyone who clones the public repository | gitleaks matched a cloud access key | You revoke the key; then it moves to the CI's secret store | None |
| 2 | High | Path traversal (CWE-22) | `src/upload.rs:41` | A signed-in user uploads a zip with an entry named `../../config.toml` | `rejects_zip_slip` writes outside the folder | Resolve each entry, and reject one that leaves the folder | Low |

Then:

1. **List what only the user can do:** revoke and reissue each exposed secret, decide whether to
   rewrite the history, change repository and provider settings, and decide how to disclose.
2. **Ask which findings to patch.** Offer to patch every confirmed finding whose patch changes
   nothing a legitimate user relies on under one approval, and ask about each of these by its
   number:
   - a traced or suspected finding;
   - a patch that changes behavior users see: stricter validation that may reject input they
     send today, a new sign-in requirement, or a cookie change that signs everyone out;
   - a patch that changes public API (see **Breaking changes**), a file format, or stored data;
   - a dependency upgrade across a major version.
3. **Ask how to disclose.** If the repository is public and the vulnerability is in a released
   version, the patch itself tells attackers where to look. Ask the user how to handle it before
   you commit: through the project's security policy (often `SECURITY.md`), or a draft security
   advisory on GitHub, which provides a temporary private fork for preparing the fix, with a
   release before the details are published.
4. **Send upstream bugs upstream.** If the vulnerability is in a dependency or in vendored code,
   the fix belongs in that project. Draft a private report for the user to send through that
   project's security policy.

## 10. Patch one finding at a time

Make each patch a complete change that one commit can describe.

1. Write a test that feeds the attack input to the vulnerable code and checks that it's refused
   or made harmless, where the **Test layout** puts tests. Run it, and watch it fail.
2. Patch the cause at the boundary, with the platform's proven tools, so that the whole class is
   closed and not only this one input:
   - parameters for queries, never escaping by hand;
   - an argument list for a process, never a shell (`execFile` in Node, `Command::new` with
     `.arg` in Rust, `subprocess.run` with a list in Python);
   - the framework's context-aware escaping for HTML, and `textContent` for text;
   - allowlists of what is valid, never denylists of known attacks;
   - paths resolved and checked against their root, and archive extraction that refuses entries
     outside it (Python's `filter="data"`);
   - parsers set to refuse what the program doesn't need: document type declarations in XML
     (`disallow-doctype-decl`), and object construction in YAML (`yaml.safe_load`);
   - the platform's cryptography library; Argon2id for passwords, with at least 19 MiB of memory,
     2 iterations, and a parallelism of 1, as the OWASP Password Storage Cheat Sheet recommends;
     and a cryptographically secure random generator (`SecureRandom`, Python's `secrets`, or
     `crypto.getRandomValues`);
   - constant-time comparison of secrets (`MessageDigest.isEqual`, `hmac.compare_digest`, or
     `crypto.timingSafeEqual`);
   - authorization checked on the server, on every request, for the object requested.
3. Run the test again, and watch it pass.
4. Search for the same pattern everywhere else, because a mistake made once is often made again
   in copied code. Add each one you find to the findings, and patch it only once the user
   approves it.

Never patch a vulnerability by hiding it: a check only in the client, an obscure URL, a denylist
of attack strings, an error caught and discarded, or a scanner rule turned off. Never weaken an
item in the **Security controls**.

**A vulnerable dependency:** upgrade it to the lowest fixed version the **Dependency policy**
allows, in a change of its own, and run every check, because an upgrade can break things too. If
no fixed version exists, give the user the options (a workaround in this code, replacing the
dependency, or accepting the risk with the reason recorded in the **Decision log**) and ask.

**A secret:** ask the user to revoke it first, because removing it from the code doesn't make it
safe. Then remove it, load it from where the language rules say secrets belong (an environment
variable, the CI's secret store, the Keychain, or the Android Keystore), and add the file that
held it to `.gitignore`.

Stop and ask if a patch grows bigger than its finding described, or needs a decision the user
hasn't made. If a finding proves wrong while you patch it, drop it, and say so in the report.

## 11. Verify

1. Run the **Check commands** (or the **Test tools**) and the **Done checks** again, and compare
   each result with the baseline: nothing newly failing, and no new warnings.
2. Confirm that each regression test failed before its patch and passes now.
3. Run every scanner that reported a finding again: the finding is gone, and nothing new has
   appeared.
4. Search once more for each pattern you patched.
5. Do the checks in "Before finishing" in `writing.md` for each comment and document you
   changed.

## 12. Record what's left

- List the findings you didn't patch, and why: not approved, waiting for a decision or for the
  user's action, or outside the scope.
- Where an unpatched vulnerability is recorded is the user's decision. In a public repository, an
  issue, a roadmap entry, or a code comment discloses it; offer a draft security advisory or a
  private note instead.
- Suggest checks that would catch each kind of finding next time: secret scanning with push
  protection, which blocks a push that contains a secret; dependency alerts; a static-analysis
  scanner and zizmor in CI; and the threat model, written down in the **Security controls**.
  Don't add one unless the user agrees.

## 13. Commit only as the project allows

- Follow the **Git access** level in `commits.md`, and leave the changes uncommitted unless the
  user asked for a commit and that level allows it. Either way, end the report with copy-ready
  commit messages, as `commits.md` describes, one for each patch. A patch to the code is a `fix`;
  a dependency upgrade is a `build` change.
- In a public repository, a commit message is public the moment it's pushed. Until the fix is
  released, say what changed without saying how to attack the old code, and ask the user before
  you name an advisory that isn't public yet.
- Before a push the user asked for, say what the **Push effect** will set off. A push that
  deploys ships the patch, and it also publishes the commit that points at the vulnerability.

## 14. Report

End with a summary that contains:

- **Scope:** what you scanned, and what you left out and why.
- **Threat model:** the assets, entry points, and attackers you worked from.
- **Baseline:** each check's and each scanner's result before you started.
- **Patched:** each finding patched, with its regression test.
- **Not patched:** each finding left, by its number, and why.
- **Dropped:** each finding that proved wrong.
- **For you to do:** each secret to revoke, history to rewrite, setting to change, and
  disclosure to decide, with the steps.
- **Checks:** each check and scanner after the patches, with its real result compared with the
  baseline, and each one you couldn't run, and why.
- **Questions:** each decision the user still has to make.
- **Prevention:** the checks you suggest in step 12.
- **Commit messages:** in `text` blocks, as `commits.md` describes, with no attribution lines.

## Tools

Use the scanners the project already runs first. Several of these download rules or advisory
data when they run, and the ground rules say to ask before a tool sends anything off the machine.

| Area | Tools |
| --- | --- |
| Secrets | gitleaks: `gitleaks git` scans the history, `gitleaks dir` scans files, and `--redact` hides the values in its output. It only matches patterns, and now receives security fixes only. TruffleHog only with `--no-verification`, because otherwise it checks each secret it finds against the provider |
| Dependencies | OSV-Scanner (`osv-scanner scan source -r .`) reads Cargo, npm, Dart, and Gradle lock files and Python requirements files, but not Swift packages, and has an `--offline` mode with a local database. The ecosystems' own audits: `cargo audit` or `cargo deny check advisories`, `npm audit`, and `pip-audit -r requirements.txt`. cargo-geiger counts the `unsafe` code in a crate's dependencies |
| Code, many languages | Semgrep (`semgrep scan --config p/default`) supports Rust, Kotlin, Swift, Java, JavaScript, TypeScript, and Python, with Dart and Lua experimental; it sends usage metrics whenever it pulls rules from its registry, unless you pass `--metrics=off`. CodeQL, through GitHub code scanning, covers Java and Kotlin, JavaScript and TypeScript, Python, Rust, Swift (analyzed on macOS only), and GitHub Actions workflows |
| Code, one language | Bandit (`bandit -r <path>`) for Python. eslint-plugin-security for JavaScript, whose findings need triage because many are false positives. Android Lint's Security checks, such as `SetJavaScriptEnabled`, `AddJavascriptInterface`, `TrustAllX509TrustManager`, `BadHostnameVerifier`, `ExportedService`, `UnsafeIntentLaunch`, and `InsecureBaseConfiguration`. Miri for undefined behavior in `unsafe` Rust |
| CI workflows | zizmor (`zizmor --offline .`), whose audits include `template-injection`, `dangerous-triggers`, `unpinned-uses`, and `excessive-permissions`. actionlint, whose checks include script injection by untrusted inputs |
| Mobile builds | MobSF analyzes built APKs and iOS app archives (IPAs), and can run from its Docker image |
