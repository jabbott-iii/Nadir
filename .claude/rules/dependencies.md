# Dependencies

These rules cover everything this repository uses that it didn't write: packages from a registry
or a Git URL, code copied or vendored into it, build plugins, test and lint tools, continuous
integration (CI) actions, container images, and files or services loaded from another site. The
default is to use none of them. A dependency comes from the **Allowed publishers**, who control
every release of it, or it isn't added. A third-party dependency, from anyone else, is allowed
only when it is impossible to avoid, and only after the user approves it.

Messages, records, and the reason for each exception follow `writing.md` in this folder. In each
language rule adopted here, such as `rust.md` or `lua.md`, set **Dependency policy** to
"`dependencies.md` in this folder", so the language rule defers to this one.

Apply these rules to dependencies you add, update, or suggest. Don't remove an existing one only
to make the repository conform; "Dependencies already here" says what to do instead.

## Project details

A repository fills in this section when it adopts the rule, and the rest of the rule refers to
each value by its bold name. If a value still reads `[FILL IN: …]`, the rule isn't set up here
yet. Work out that value from the repository itself, and tell the user which values are missing
so they can fill them in. Until **Allowed publishers** is filled in, treat every package as third
party, and until **Approved exceptions** is filled in, treat it as "none".

- **Allowed publishers:** the user's own accounts, <https://github.com/jabbott-iii> and
  <https://crates.io/users/jabbott-iii> (ADR-0084, which widened ADR-0079's rule for Ferrolua's
  crates). A crate is first party only while every owner crates.io lists for it is the user's
  account, with no team, its source repository is under <https://github.com/jabbott-iii>, and its
  own tree holds only first-party crates, whether or not it uses `unsafe` (ADR-0083). The user
  approves each one in the change that adds it, from crates.io by an exact pin, and the
  dependency matrix in `docs/01-architecture.md` names it in the row of each crate that uses it.
  Ferrolua's crates go in `nadir-script` alone, and Nadir never links Ferrolua's JIT. Every other
  package from outside the workspace is third party (`AGENTS.md`, rule 1;
  `docs/02-dependency-policy.md`).
- **Platform:** Rust's toolchain as `rust-toolchain.toml` pins it (rustc, Cargo, rustfmt, and
  Clippy) and its standard library; the operating systems' own libraries and frameworks, which
  `nadir-sys` binds by hand, and the OS and SDK tools that `xtask` packaging runs (both listed in
  `docs/02-dependency-policy.md`); GitHub Actions, with only the actions that
  `cargo xtask workflows` allows, each pinned to a full commit SHA (ADR-0032); and the Ubuntu
  packages the workflows install with `apt-get` (Mesa's drivers, `libegl1`, `libasound2t64`,
  `dpkg-dev`, and GStreamer, for the demo page's videos). LDtk and Aseprite, whose files the cook
  reads, are tools a person uses, not dependencies.
- **Manifests:** the root `Cargo.toml` and each member's, `Cargo.lock`, `rust-toolchain.toml`,
  `.cargo/config.toml`, the `uses:` and `apt-get install` lines in `.github/workflows/`,
  `.github/dependabot.yml`, and the `FROM` lines in `tools/cook/Dockerfile`. There is no
  `.gitmodules`.
- **Tree commands:** `cargo tree --workspace --edges normal,build,dev --target all --prefix none`,
  and `cargo xtask deps`, which fails if a package from outside the workspace is not in
  `xtask/deps-allowlist.txt` at its exact version.
- **Approved exceptions:** the build-or-buy register in `docs/02-dependency-policy.md`, its rows
  of status `Approved`, each also listed in `xtask/deps-allowlist.txt` with its version and its
  decision record: none today. First-party crates are not exceptions, but are listed in
  `xtask/deps-allowlist.txt` too, Ferrolua's with ADR-0079 and the rest with ADR-0084, so that
  `cargo xtask deps` passes them; none is listed yet. `/evaluate-dependency` writes the
  evaluation that the policy asks for, for the user to decide on.

## What counts as a dependency

Anything the project builds with, runs, tests with, or ships that it didn't write counts:

- a package from a registry, such as crates.io, npm, the Python Package Index (PyPI), or
  LuaRocks, or one fetched from a Git URL
- every package that another one brings in, however deep in the tree
- dev dependencies, build plugins, code generators, and the test, lint, and format tools that the
  project or its CI installs
- the packages that a template or a project generator, such as `npm create`, adds
- code copied in: a vendored folder (a copy of another project kept in this repository), a Git
  submodule, or a file, function, or snippet from another project, a tutorial, or an answer online
- anything fetched while the project builds or runs: a CI action, a container base image, an
  install script piped to a shell, a binary that a build script downloads, or a script,
  stylesheet, or font loaded from another site
- a hosted service the code calls, such as another company's API or an analytics script

Copying code into the repository doesn't make it first party. It is still someone else's code,
and its license comes with it.

These don't count:

- the **Platform**: the language, its compiler or interpreter, its standard library, its package
  manager, the operating system, and the SDKs and services the project is built on
- packages from the **Allowed publishers** that pass the checks in the next section, though the
  packages they bring in still count
- tools that a person uses but the project doesn't require, such as an editor

## When a package is first party

A package is first party only when the **Allowed publishers** control every release of it. Check
both of these before you treat it as first party:

- **Its owners.** Every owner the registry lists is one of the **Allowed publishers**. Any owner
  can publish a new version, so a single other owner makes the package third party. A team
  counts only when its organization is one of the **Allowed publishers**, because every member
  of the team can publish.
- **Its source.** The repository it is built from is under an account in the **Allowed
  publishers**. A Git dependency names its repository directly; pin it to a full commit hash,
  not a branch or a tag, which can be moved to other code.

Read the owners from the registry, not from the package's own metadata. The author and repository
fields in a manifest hold whatever the publisher typed, so they can name anyone.

- On crates.io, `cargo owner --list <crate>` prints one owner per line: a person's login, or a
  team written as `github:<organization>:<team>`.
- On npm, `npm owner ls <package>` prints each owner.
- On PyPI and other registries, read the maintainers listed on the package's page.

Check the exact name, too. A package whose name is one typo away from a first-party one, or that
sits under a look-alike account, belongs to whoever the registry lists as its owners. Publishing
under near-miss names (typosquatting) is a common way to get malware installed.

## Before you add anything

Look for a way to do the job in this order, and stop at the first one that works:

1. The **Platform**: the standard library, the SDK, or a tool that ships with the toolchain.
2. Code the project already has.
3. A package from the **Allowed publishers** whose whole tree is first party, part of the
   **Platform**, or in **Approved exceptions**.
4. Code you write in this repository: only what the project needs, with tests, in a module of its
   own. If other projects could use it too, suggest that the user publish it under one of the
   **Allowed publishers**, so the next project can depend on it.
5. A third-party package, only when every step above is impossible. "When a third-party
   dependency is allowed" says what impossible means, and "Ask before you add one" says what to
   do next.

Writing the code here is the default, even when a package would be faster. If it would be a large
piece of work, say so, with an estimate of its size, and let the user decide how to go on. Size
alone doesn't make it impossible.

## When a third-party dependency is allowed

A third-party dependency is impossible to avoid only for one of these reasons:

- **The platform requires it.** The project can't build, run, or ship on the **Platform** without
  it. For example, Google Play's Payments policy requires most apps that sell digital goods to use
  Google Play's billing system, which an app reaches through Google's Play Billing Library.
- **Only its owner can talk to the thing.** A file format, protocol, device, or service has no
  public specification, so only its owner's library can read it, speak it, or drive it.
- **A law, a contract, or a certification names it.** For example, a contract requires a
  cryptographic module validated under Federal Information Processing Standard (FIPS) 140-3.
- **A homemade version would be a security hole.** This covers cryptography, such as ciphers,
  password hashing, and random numbers for keys, and the protocols built on it, such as Transport
  Layer Security (TLS), when the **Platform** doesn't provide them. Writing these is possible,
  but homemade versions are a well-known source of vulnerabilities, so treat them as impossible
  to write here.

None of these is a reason on its own:

- it is popular, or the usual choice in its ecosystem
- it saves time, or the code it replaces would be long
- it is small or well maintained, or a large company publishes it
- a tutorial, an answer online, an example, or another rule uses it
- a request mentions it, until the user confirms they want an exception for it

## Ask before you add one

Stop before you add a third-party dependency, and ask the user. Tell them:

- what the project needs, and why each step in "Before you add anything" fails
- which reason in "When a third-party dependency is allowed" applies, and the evidence for it,
  such as the policy, the contract, or the missing specification
- the package's exact name and version, its owners on the registry, and its license
- how many packages it brings in, from the **Tree commands**, and which of those are third party
- the smallest way to use it: the fewest features (in Cargo, `default-features = false` and only
  the features the code needs), and one module of the project's own that wraps it

Once the user approves it:

- Record it in **Approved exceptions**, with its version and the reason, in the same change that
  adds it.
- Pin it. Keep the lock file in the same change, and pin a Git dependency or a CI action to a
  full commit hash, so the code that was checked is the code that builds.
- Keep it behind that one module, so replacing it later changes one place.

An approval covers that package, for that reason, within one major version. Cargo and npm treat
a change in the first nonzero number as breaking, so for a version below 1.0, such as 0.4.2,
that means within 0.4. A new package, a new major version, or a change that brings in more
packages needs an approval of its own.

## Updates

- An update can bring in new packages. Run the **Tree commands** before and after any update,
  including one a bot such as Dependabot proposes, and compare the two lists.
- When an update to a first-party package brings in a third-party one, that package is a new
  third-party dependency, and it needs an approval.
- When an approved exception has a security fix, propose the update as soon as you find it.

## Dependencies already here

A repository that adopts this rule may already have third-party dependencies that aren't in
**Approved exceptions**. When you find one:

- Leave it in place. Removing or replacing it is a change of its own, and the user decides it.
- Tell the user what it is, what uses it, and whether the **Platform**, a package from the
  **Allowed publishers**, or code of the project's own could replace it.
- Don't add new uses of it while the user decides.

## Before finishing

1. Run the **Tree commands**, and check that every package they list is part of the **Platform**,
   from the **Allowed publishers**, or in **Approved exceptions**. List the packages for every
   platform the project builds for, not only the one you're on: `cargo tree`, for example, leaves
   out another platform's dependencies unless you pass `--target all`.
2. Check each of the **Manifests** you changed for what the **Tree commands** don't list: a
   `uses:` line, a `FROM` line, a Git URL, a submodule, or a URL a build script downloads from.
3. Check that each changed lock file is in the same change as its manifest.
4. Check the text you wrote, such as install steps, examples, and templates, for a third-party
   package it tells a reader to install.
5. In your report, list every dependency you added, updated, or removed, and say how each passes
   this rule.
