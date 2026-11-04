# Security Policy

## Supported versions

Nadir is in development. A tag `v<version>` publishes a release through
`.github/workflows/release.yml` (see "Engine releases" in
[`docs/05-release.md`](docs/05-release.md)): programs on the GitHub
release, an apt repository, a container image and crates on crates.io.
There are no maintenance branches. Security fixes land on `main`, and
reach users in the next release.

| Version | Supported |
| :- | :- |
| `main` | :white_check_mark: |
| The newest release, once there is one | :white_check_mark: |
| The sandbox download on the [demo page](https://jabbott.dev/Nadir/), rebuilt from each commit of `main` that passes CI | :white_check_mark: (the latest one) |
| Earlier commits, downloads and releases | :x: |

## Reporting a vulnerability

Report vulnerabilities privately, through GitHub's private vulnerability
reporting:

1. On the repository's **Security** tab, select **Report a
   vulnerability**, or go straight to
   <https://github.com/jabbott-iii/Nadir/security/advisories/new>.
2. Fill in the form. Only you, the maintainer, and the collaborators the
   maintainer adds to the report can see it.

Do not report a vulnerability in a public issue, discussion, pull request
or commit message.

Include as much of this as you can:

- The commit (`git rev-parse HEAD`), or the build the demo page names.
- The OS and its version, the CPU architecture, and for rendering or
  window problems the GPU and its driver (the log names the OpenGL
  renderer).
- The input that triggers it: attach a small file (an image, a sound, a
  JSON or KBJ file, a save) to the private report rather than linking to
  it.
- The steps to reproduce it, or a proof of concept. A failing test, or a
  crash from `cargo xtask fuzz-smoke`, is ideal.
- The impact: what an attacker has to control, and what they gain.
- A suggested fix, if you have one.

Remove user names, paths and other private data from any log or output
you attach.

## What to expect

Nadir has a single maintainer, so responses are best effort. A report goes
through these steps:

1. **Acknowledgement.** The maintainer replies in the private report.
2. **Assessment.** The maintainer confirms whether it is a vulnerability,
   which commits it affects and how severe it is, and discusses it with you
   there.
3. **Fix.** The fix is prepared privately, in the advisory's temporary
   private fork when needed, and lands on `main`. The demo page publishes
   a new download once CI passes on that commit.
4. **Disclosure.** Once the fix is on `main`, the maintainer publishes a
   GitHub security advisory, credits you unless you would rather not be,
   and requests a CVE through GitHub when one is warranted.

If a report is declined, for example as out of scope, the maintainer
explains why in the report. Please keep the details private until the
advisory is published, or until you and the maintainer agree on a date.

## Scope

Nadir treats every file it reads as possibly malformed or hostile: saves,
mods, downloaded content and corrupted files. In scope:

- **Parsers and loaders**: the codecs in `nadir-codec` (inflate, PNG, QOI,
  WAV, JSON and JSONC, KBJ, KLZ, BMFont, Aseprite exports, LDtk projects,
  and the atlas, level, prefab and pack files), the asset server and its
  sources in `nadir-assets`, and, as the roadmap adds them, QOA sound and
  saves. For example:
  - a panic, a hang, or a stack overflow on any input;
  - memory use past the limits a parser is given (`Limits`);
  - an asset path that reaches outside the asset folder, such as one with
    `..` or an absolute path.
- **`unsafe` code and OS bindings** in `nadir-sys`, `nadir-platform` and
  `nadir-gfx`: undefined behavior that safe code can reach, such as a
  binding whose layout does not match the OS's, or a safe wrapper that is
  not sound.
- **The asset cook** (`nadir-cook`): writing or deleting outside its
  output folder, reading outside its source folder, or following links.
- **Build automation and CI**: `xtask`, which must never reach the network
  or read secrets, and the workflows in `.github/workflows/`, such as
  script injection, an unpinned action, or a token with more permissions
  than its job needs.
- **The demo page and its download**: what `cargo xtask site` builds and
  `.github/workflows/pages.yml` publishes to <https://jabbott.dev/Nadir/>,
  such as HTML or script injection from the build's data, or a download
  that does not match the commit and checksum the page names. The page
  also loads jabbott.dev's own stylesheet and script, which run on the same
  site: report a problem in those here too, through the same private
  form. The maintainer runs both sites, and fixes it in jabbott.dev's
  repository.

Out of scope:

- **A game's own code.** A game built on Nadir runs with its player's
  privileges; what its code does is not an engine vulnerability.
- **Code behind the `dev` feature**, such as the placeholders for missing
  assets and, as the roadmap adds them, the debug tools in `nadir-debug`.
  It is for development, and `cargo xtask release-no-dev` checks that
  release builds leave the debug tools out: report a problem only `dev`
  code can reach as an ordinary issue. A release build that includes it is
  in scope.
- **Bugs in the OS, GPU drivers, Mesa, GStreamer, GitHub Actions or the
  Rust toolchain.** Report those to their maintainers, and tell us if
  Nadir needs to work around one.
- **The cost of a large file within the limits**, such as the time a valid
  image at the size limit takes to decode.
- **Attacks that need control of the player's account**, such as
  replacing the game's executable or its libraries.
- **The lack of code signing on the demo page's download and on the
  releases' programs.** Verify the download with the SHA-256 the page
  gives, and a release's files with its `SHA256SUMS`. Signing, and
  notarization on macOS, come with roadmap task M11.2
  (`docs/05-release.md`).

Nadir has no third-party crates (see
[`docs/02-dependency-policy.md`](docs/02-dependency-policy.md)), so there
are no dependency advisories to track beyond Rust's standard library.

## Verifying a download

The demo page names the commit each download was built from, and gives its
SHA-256. Check it before you run it:

```bash
sha256sum nadir-sandbox-linux-x86_64.tar.gz
```

Compare the result with the hash on the page. The download is built by the
workflow in `.github/workflows/pages.yml` from that commit, after CI passed
on it.

A release's files are listed with their SHA-256 in its `SHA256SUMS`:

```bash
sha256sum --check --ignore-missing SHA256SUMS
```

The apt repository is signed: apt refuses an index that the key in
`nadir-archive-keyring.gpg` did not sign. Compare that key's fingerprint
(`gpg --show-keys nadir-archive-keyring.gpg`) with the one recorded in
`docs/05-release.md` before you trust it. None is recorded there yet: the
key will be made, and its fingerprint recorded there, before the first
release. Each release is built by `.github/workflows/release.yml` from a
commit of `main` that CI passed.

## How the code is kept safe

- `unsafe` is allowed in three crates only, and `cargo xtask unsafe-audit`
  fails any `unsafe` block without a `// SAFETY:` comment.
- Each parser in `nadir-codec` has a fuzz target, and CI runs each for 30
  seconds on every push and pull request (`cargo xtask fuzz-smoke`).
- `cargo xtask deps` fails the build if `Cargo.lock` gains a package from
  outside the workspace.
- CodeQL scans the Rust code and the workflows with its
  `security-extended` queries, on every push to `main`, every pull request
  and each week.
- Every workflow pins each action by its full commit SHA and uses only the
  actions `docs/04-quality.md` allows it. Its token reads the repository
  by default, and only the jobs that need it are given write access, to
  what they need: CodeQL's analysis job to code scanning, the demo page's
  deploy job to Pages, the release workflow's `release` job to the
  repository's contents (`contents: write`, which it needs to publish the
  release, though it could also push to any branch), and its `image` job
  to its packages. `cargo xtask workflows` checks this on every push, and
  Dependabot proposes updates to the pins each week.

## Known issues

Vulnerabilities that were reported are disclosed as GitHub security
advisories once fixed (see [What to expect](#what-to-expect)). Published
advisories are listed at
<https://github.com/jabbott-iii/Nadir/security/advisories>.
