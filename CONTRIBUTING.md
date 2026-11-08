# Contributing to Nadir

Thanks for your interest in Nadir, a 2D top-down game engine for desktop,
written in Rust with no third-party crates. This guide covers how to
propose a change, set up a machine, check your work and open a pull
request.

The project's rules are in [`AGENTS.md`](AGENTS.md), which every
contributor follows, person or coding agent. Where this guide and
`AGENTS.md` disagree, `AGENTS.md` wins. The design is in [`docs/`](docs):
start with [`docs/00-overview.md`](docs/00-overview.md), then the
subsystem spec of the crate you are changing (the table "Read before
working" in `AGENTS.md` says which).

## Contribution workflow

1. **Open an issue first,** with the issue form: a bug, a proposal, a
   question, or a roadmap task you would like to take. A pull request with
   no issue will be declined, unless it fixes a typo or a broken link.
2. **If an issue already exists,** comment on it instead of opening
   another.
3. **Wait for a maintainer to confirm** the change on the issue before you
   open a pull request.
4. **Branch from `main` and make the change,** keeping to the one issue.
5. **Check your work** (see [Checks](#checks)), then open a pull request
   that links the issue. The template asks what you ran.
6. **CI must pass** on Linux, Windows and macOS. A maintainer reviews every
   pull request (see `CODEOWNERS`).

### Roadmap tasks

Most work is a task in [`docs/roadmap.md`](docs/roadmap.md). Each task says
what to do (`Do`), what must be true when it is done (`Accept`) and how to
check it (`Verify`), and lists the tasks it depends on. Take the first
unticked task whose dependencies are all ticked, unless the issue agreed
on another. A task is done when:

1. every acceptance criterion is met;
2. its `Verify` commands ran and passed, and the pull request reports each
   with its real result;
3. its checkbox is ticked, and only then;
4. the docs are up to date: the subsystem spec where behavior or design
   changed, an ADR in [`docs/decisions.md`](docs/decisions.md) where a
   decision changed, and the task's feature in
   [`docs/features.jsonc`](docs/features.jsonc), whose summary describes
   what is built now (`cargo xtask features` checks the ticks).

A criterion that needs an OS or hardware you do not have stays unticked,
with a `⏳ needs <OS>: <the remaining check>` line under the task.

## Ground rules

These are the rules in `AGENTS.md` that a pull request is most often
held up on:

- **No third-party crates.** Do not add a crate from outside this
  workspace to any dependency table. If you think one is needed, say so on
  the issue, with the evaluation that
  [`docs/02-dependency-policy.md`](docs/02-dependency-policy.md)
  describes. `cargo xtask deps` enforces this.
- **Layering is one-way.** A crate may depend only on the crates its row of
  the dependency matrix in
  [`docs/01-architecture.md`](docs/01-architecture.md) lists.
  `cargo xtask layers` enforces this.
- **`unsafe` lives in `nadir-sys`, `nadir-platform` and `nadir-gfx`
  only.** Each `unsafe` block has a `// SAFETY:` comment saying why it is
  sound, each `pub unsafe fn` a `# Safety` section, and raw OS
  declarations live only in `nadir-sys`. `cargo xtask unsafe-audit` checks
  that `unsafe` stays in those crates and that each block has its comment,
  clippy (`undocumented_unsafe_blocks`, `missing_safety_doc`) checks the
  comments and `# Safety` sections too, and review checks where
  declarations live.
- **Platform `cfg` stays in a few places:** `#[cfg(target_os = ...)]`
  appears only in `nadir-sys`, `nadir-platform`, the backend selection in
  `nadir-gfx`, and the packaging code in `xtask`. Game-facing APIs are the
  same on every OS.
- **Smallest complete change.** No unrelated refactors, renames or
  reformatting. Keep public APIs unless the issue agreed to break one, and
  record a break in `docs/decisions.md`.
- **Report what you ran, honestly.** Never say a check passed unless you
  ran it and saw it pass, and say which checks you did not run.
- **No secrets.** Never commit signing identities, certificates, tokens or
  store credentials. Release secrets live only in CI.
- **Scope.** Web and WASM, Android, iOS, consoles, 3D and networking are
  out of scope ([`docs/00-overview.md`](docs/00-overview.md), "Non-goals").
  A new subsystem, or anything else beyond the roadmap, needs an ADR
  first.

## Development setup

### Toolchain

- **Rust through [rustup](https://rustup.rs).** `rust-toolchain.toml` pins
  the version and the `rustfmt` and `clippy` components; rustup installs
  them the first time you run `cargo` in the repository.
- **Git.**

### Per system

| System | You need |
| :- | :- |
| Linux (x86_64 or arm64) | An X11 session, or Wayland with XWayland, and OpenGL 3.3 through EGL. The engine loads `libX11`, `libEGL` and the GL library at run time, so no development packages are needed. For the OpenGL tests and golden images without a GPU: `libegl1` and `libgl1-mesa-dri` (Mesa's llvmpipe). |
| Windows 10+ (x64 or Arm) | The MSVC build tools and the Windows SDK (Visual Studio Build Tools). |
| macOS 12+ | The Xcode command-line tools (`xcode-select --install`). Full Xcode is not needed. |

Only Linux runs the engine so far: the Windows and macOS windows and
OpenGL contexts are roadmap tasks M1.5, M1.6, M2.6 and M2.7. Everything
else builds and tests on all three.

Optional:

- **GStreamer** (`gstreamer1.0-tools`, `-plugins-base`, `-plugins-good` on
  Ubuntu), to build the demo page with `cargo xtask site`.

### Build and run

The sandbox draws cooked sprites: cook `assets/` into `target/cooked`
first, and again after changing them. `cargo xtask ci` cooks before its
tests, which need the cooked sprites too; run `cargo xtask cook` before
`cargo test` on its own.

```bash
cargo xtask cook
cargo run -p sandbox
cargo run -p sandbox -- --scene stress-sprites
cargo dev
```

`cargo dev` runs the sandbox with the `dev` feature: a development build,
where a missing asset shows a placeholder, and where the debug tools will
go (`nadir-debug`). WASD or the arrow keys walk. With
`cargo xtask cook --watch` running beside it, an edit to `assets/`, or to
the shaders in `crates/render/shaders/`, shows in the running game within
a second, but for what the game copied at setup (the sandbox's crate
docs say which).

## Commands

Every check and tool is a `cargo xtask` command, the same on Linux,
Windows and macOS, and the same commands CI runs:

| Command | What it does |
| :- | :- |
| `cargo xtask ci` | The whole pull request gate: fmt check, clippy with `-D warnings`, `cook`, tests, `deps`, `layers`, `crates`, `unsafe-audit`, `headers`, `workflows`, `release-no-dev`, `features` |
| `cargo xtask deps` | Fails if `Cargo.lock` holds a package from outside the workspace that the policy does not approve |
| `cargo xtask layers` | Fails if a crate depends on one the dependency matrix forbids |
| `cargo xtask crates` | Fails if a crate for crates.io lacks what crates.io shows or copies of `LICENSE` and `NOTICE`, is at another version, or depends on a crate crates.io does not get |
| `cargo xtask unsafe-audit` | Fails if `unsafe` appears outside the three allowed crates, or without a `SAFETY:` comment |
| `cargo xtask headers` | Fails if a source file does not start with the license header, in its place |
| `cargo xtask workflows` | Fails if a workflow in `.github/workflows` uses an action it may not, pins one by anything but a commit SHA with its tag in a comment, or gives its token more than it may |
| `cargo xtask release-no-dev` | Fails if `nadir-debug` is in the sandbox's dependency tree without the `dev` feature |
| `cargo xtask features` | Fails if `docs/features.jsonc` does not match the roadmap's ticks |
| `cargo xtask golden [--bless]` | Draws the golden scenes offscreen and compares them with `tests/golden/` |
| `cargo xtask fuzz-smoke [--secs N] [--target T]` | Runs the mutation fuzzer over every parser, or one |
| `cargo xtask bench [filter]` | Runs the benchmarks in release, against the recorded baseline |
| `cargo xtask cook [--force] [--watch]` | Cooks `assets/` into `target/cooked/`; `--watch` again whenever they change, until stopped |
| `cargo xtask site [--out DIR]` | Builds the demo page into `target/site` (needs EGL and GStreamer) |
| `cargo xtask dist [--out DIR]` | Builds this machine's release files into `target/dist`: the archive and, on Linux, the `.deb`s (needs `dpkg-dev`) |

### The Makefile

The root `Makefile` has one target, `make release`, with which a
maintainer tags a release and pushes the tag
([`docs/05-release.md`](docs/05-release.md), "Making a release"). Every
other check and tool is a `cargo xtask` command, written in Rust in this
workspace: it runs the same way on all three systems, including Windows,
where `make` is not installed by default, and CI runs the same commands.
The Makefile leaves them out, as it would be a second list of the same
commands to keep in step. If you are used to `make`:

| Instead of | Run |
| :- | :- |
| `make check` | `cargo xtask ci` |
| `make fmt` | `cargo fmt --all` |
| `make lint` | `cargo clippy --workspace --all-targets --all-features -- -D warnings` |
| `make test` | `cargo xtask cook`, then `cargo test --workspace --all-features` |
| `make bench` | `cargo xtask bench` |
| `make run` | `cargo xtask cook`, then `cargo run -p sandbox` |

## Checks

Run these before you open a pull request:

```bash
cargo fmt --all
cargo xtask ci
```

On a machine without a GPU, or to match CI on Linux, run the checks on
Mesa's software renderer:

```bash
LIBGL_ALWAYS_SOFTWARE=1 NADIR_EXPECT_GL_RENDERER=llvmpipe cargo xtask ci
```

Then, as the change needs:

- **Rendering, `gfx`, `platform` or the scenes:** `cargo xtask golden`,
  on llvmpipe as above. The references were drawn there, and
  [`docs/04-quality.md`](docs/04-quality.md) says how to choose Mesa where
  NVIDIA's EGL is also installed. Re-bless a reference only when the
  change means to alter what is drawn, and say why in the pull request.
- **A parser (`codec`, `assets`, `scene`, the cook):**
  `cargo xtask fuzz-smoke --target <parser>`. A new parser registers a
  fuzz target with seeds in `fuzz/corpus/<target>/` and hand-made test
  vectors.
- **Performance:** `cargo xtask bench <filter>`, with the numbers before
  and after in the pull request.
- **The cook or `assets/`:** `cargo xtask cook`.
- **The demo page:** `cargo xtask site --out <a new folder>`, and look at
  the page.

CI runs `cargo xtask ci` on Linux, Windows and macOS, and the golden
images and fuzzing on Linux. If your change touches Windows or macOS code
and you cannot run that system, say so in the pull request.

## Coding conventions

- **Edition and toolchain:** Rust 2024 on the pinned stable toolchain.
  `extern` blocks are written `unsafe extern "C" { ... }`.
- **Coordinates:** world space is +X right and **+Y down**, in pixels at
  zoom 1. Angles are radians, and positive rotation is clockwise on screen.
- **Time:** the simulation runs at a fixed 60 Hz tick, and rendering
  interpolates between the last two ticks.
- **Errors:** each crate has its own error enum implementing `Display` and
  `std::error::Error`. No `unwrap()` in library code; use `expect("why
  this cannot fail")` only for a real invariant.
- **Logging:** the `nadir_core::log` macros (`error!` to `trace!`), never
  `print!` or `eprint!` in a library crate.
- **Allocation:** no heap allocation per frame in steady-state rendering,
  ECS iteration, audio callbacks or collision. Reuse buffers. The dev
  allocation counter checks this.
- **Docs:** every `pub` item has a doc comment.
- **Lints:** never weaken `[workspace.lints]`, `clippy.toml` or
  `unsafe_code`. An `#[allow(...)]` needs a comment saying why the lint is
  wrong there.
- **Parsers:** never panic on input, check every size against `Limits`
  before allocating, bound recursion, and give errors with the offset or
  line. On-disk format changes bump the format's version. See
  [`.claude/rules/parsers.md`](.claude/rules/parsers.md).
- **OS bindings:** declared in `nadir-sys` with layout assertions and a
  citation of the official header, and wrapped safely in `nadir-platform`
  or `nadir-gfx`. See
  [`.claude/rules/unsafe-and-ffi.md`](.claude/rules/unsafe-and-ffi.md).

## Tests

- Unit tests live in a `#[cfg(test)] mod tests` beside the code, and
  integration tests in each crate's `tests/`.
- Add or update tests for every change in behavior. They must be
  deterministic: seed any randomness, and print the seed of a failure.
- Never delete or `#[ignore]` a test, raise a tolerance, or re-bless a
  golden image to make a check pass. If a test is wrong, say why in the
  pull request.
- Golden images live in `tests/golden/`, and determinism hashes beside
  their tests: when a change means to alter them, bless them again and say
  why.

## Documentation

Update the docs in the same pull request when the change affects them:

- The subsystem spec in `docs/subsystems/` for a change in behavior or
  design.
- An ADR in `docs/decisions.md` for a decision made or changed. Use the
  template at the top of that file; to supersede an ADR, change only its
  `Status` line.
- `docs/roadmap.md` and `docs/features.jsonc` for a roadmap task.
- `AGENTS.md` for a new command, crate or rule.
- This file, `SECURITY.md`, `.github/pull_request_template.md` and
  `.github/ISSUE_TEMPLATE/` when the workflow, the checks or the reporting
  process change.

## Commits and pull requests

- A commit for a roadmap task starts with its ID and title, as in
  `M5.4 — cook: CLI, cache, textures, atlas packer`. Any other commit
  starts with the area it changes, as in
  `bench: qoi_decode_2048 and inflate_10mb`.
- Keep a pull request to one issue or task. Split unrelated work into
  issues of its own.
- Do not commit build output (`target/`), captures or other generated
  files the docs do not ask for.

## Coding agents

The repository is set up for coding agents: `AGENTS.md` holds the rules
for any agent, and `CLAUDE.md` and `.claude/` add Claude Code's skills
(`/next-task`, `/evaluate-dependency`, `/add-ffi-binding`,
`/record-decision`), path-scoped rules and permission guardrails.

- Agents never commit, push or rewrite history: they leave their changes
  uncommitted for a person to review.
- You are responsible for every line you submit, whoever or whatever wrote
  it. Review an agent's change as you would anyone's, and run the checks
  yourself.

## Security issues

Do not report a vulnerability in an issue or pull request. Report it
privately, as [`SECURITY.md`](SECURITY.md) describes; that file also says
what is in scope.

## License

By contributing, you agree that your contributions are licensed under the
Apache License 2.0 (see [`LICENSE`](LICENSE), section 5).

Every source file starts with the license header from the appendix of
`LICENSE`, in that file's comment syntax; copy it from any file of the
same kind into a new one. It goes after a `#!` line, an HTML doctype, or
a shader's `#version 330 core`, which must stay the first line, and
`cargo xtask headers` checks it. A crate published to crates.io keeps
copies of `LICENSE` and `NOTICE` in its folder, which `cargo xtask crates`
checks.
[`NOTICE`](NOTICE) holds only the notices a license requires there;
material that is not the project's own keeps its own license and notice
beside it.
