# 05 — Build, release, operate

## Cargo profiles

The root `Cargo.toml` sets only `[profile.xtask]` so far. The rest of this
block is the plan, which nothing sets yet. Until it is set, `cargo xtask
dist` and the release workflow build in cargo's default `release` profile,
and the `dist` profile is for `xtask package` (M11.1).

```toml
[profile.dev]
opt-level = 1                  # debuggable but not glacial

# Hot, rarely-stepped-through crates can be optimized harder in dev.
# Decide per crate by measuring sandbox frame time:
# [profile.dev.package.nadir-codec]
# opt-level = 3

[profile.release]
opt-level = 3
lto = "thin"
codegen-units = 1
debug = "line-tables-only"     # enough for symbolicated backtraces
split-debuginfo = "packed"     # .pdb / .dSYM / .dwp beside the binary: archive, don't ship
panic = "unwind"               # keep unwinding so the panic hook can write a report and
                               # catch_unwind works in the fuzz harness

[profile.dist]
inherits = "release"
lto = "fat"

[profile.xtask]                # what `cargo xtask` builds with (ADR-0028)
inherits = "dev"
opt-level = 3                  # the fuzzer tries more inputs; overflow checks and
                               # debug assertions stay on
```

> The common `[profile.dev.package."*"]` trick does nothing here. `"*"`
> matches only packages that are not workspace members, and we have none.
> Optimize heavy members explicitly, as in the commented example above.

**Compile-time hygiene:**

- Use a fast linker. `lld` is the default on recent x86_64 Linux toolchains;
  otherwise configure `mold` or `lld` in `.cargo/config.toml` per developer
  machine, not in the repo.
- Keep generic-heavy code out of hot compile paths.
- Measure `cargo build --timings` at every milestone and record notable
  regressions in the milestone notes.

## Versioning

- **Engine crates:** one workspace version, following semver from 1.0. Before
  1.0, minor bumps may break APIs and are recorded in `docs/decisions.md`.
  It is `[workspace.package]`'s `version` in the root `Cargo.toml`, which
  each `version` in `[workspace.dependencies]` repeats (`cargo xtask
  crates` checks they agree). Its release is the tag `v<version>`
  ("Engine releases" below).
- **Game builds:** the version comes from the workspace version, plus the git
  short hash, which `xtask` injects through an environment variable read with
  `env!`.
- **On-disk formats** (pack, level, save, settings): each has a `u16` format
  version.
  - Loaders support the current version and every older version that has a
    migration function.
  - Migrations are unit-tested with stored fixture files.

## Packaging (`cargo xtask package <os>`)

`xtask package` builds, assembles and (in CI only) signs. It never uploads.

### Windows

1. `cargo build --profile dist --target x86_64-pc-windows-msvc`.
2. Compile `game.rc` (icon, `VERSIONINFO`) with `rc.exe` from the Windows SDK
   into a `.res`, and link it via `build.rs` `cargo:rustc-link-arg`.
3. Embed the manifest (`/MANIFEST:EMBED /MANIFESTINPUT:...`): DPI
   PerMonitorV2, UTF-8 active code page, supported-OS GUIDs.
4. Lay out the folder: `Game.exe`, `game.pak`, `LICENSES.txt`.
5. **Sign (CI):**
   `signtool sign /fd SHA256 /tr <RFC3161 timestamp URL> /td SHA256 ...`.
   The certificate comes from CI secret storage or a cloud HSM / signing
   service.
6. Output: `Game-<ver>-windows-x64.zip`. Steam and itch.io don't need an
   installer. An installer (Inno Setup or WiX, both external tools) is
   backlog.
7. Archive `Game.pdb` with the build hash.

### macOS

1. Build `aarch64-apple-darwin` and `x86_64-apple-darwin` (dist), then
   `lipo -create` them into a universal binary.
2. Assemble `Game.app/Contents/{MacOS/Game, Resources/game.pak, Resources/Game.icns, Info.plist}`.
   Make the `.icns` with `iconutil` from an iconset folder.
3. **Sign (CI):**
   `codesign --force --options runtime --timestamp --sign "Developer ID Application: …" Game.app`.
   Add entitlements only if needed; none are expected.
4. **Notarize (CI):**
   - Zip the app with `ditto -c -k --keepParent`.
   - `xcrun notarytool submit … --wait`, using an App Store Connect API key
     from secrets.
   - `xcrun stapler staple Game.app`.
5. Output `Game-<ver>-macos-universal.dmg`, made with
   `hdiutil create -format UDZO`. Sign and notarize the DMG as well.
6. Archive the `.dSYM` bundles for both architectures.

### Linux

1. Build `x86_64-unknown-linux-gnu` (dist) **inside the oldest supported
   distro container** (glibc floor).
2. Lay out: `game/` containing `Game` (binary), `game.pak`,
   `game.desktop`, `game.png` and `LICENSES.txt`.
3. Output `Game-<ver>-linux-x64.tar.gz`.
4. Optional: build an AppImage with `appimagetool`.
5. Steam: build against the Steam Linux Runtime and test inside it.
6. Archive the `.dwp` / debug file.

The demo page's download (ADR-0029) is not this package. It is the
sandbox in the `release` profile with its cooked assets in one pack,
`assets/game.kpak` (ADR-0041), built on
`ubuntu-latest` for each push to `main` that passes CI, and unsigned. It
needs the glibc that its executable's symbol versions name, which the page
and its README state (2.34 on 2026-10-31), not the container's floor above.
Once `xtask package` exists (M11.1), the page can offer the package above
instead.

`LICENSES.txt` lists our own license and `NOTICE`, which a game that
ships the engine must carry (the license's section 4(d)), and the
licenses of any approved dependencies. With zero dependencies, that's
just ours. Fonts and other content have their own entries.

## Engine releases (`.github/workflows/release.yml`, ADR-0036)

A pushed tag `v<version>` publishes the engine at that version: the
programs, a signed apt repository, the cook's container image and the
crates. Nothing is signed or notarized until M11.2.

### What a release publishes

| Where | What |
| :- | :- |
| The GitHub release `v<version>` | `nadir-<version>-<target>.tar.gz` (Linux, macOS) or `.zip` (Windows) for each of the six targets, `{x86_64,aarch64}-{unknown-linux-gnu,pc-windows-msvc,apple-darwin}`. Each holds `nadir-cook`, the sandbox with its cooked atlases, prefabs and levels in one pack, `assets/game.kpak` (ADR-0041), `README.txt`, `LICENSE` and `NOTICE` (ADR-0037). On Windows and macOS the sandbox runs in an empty window until the engine draws there (M2.6, M2.7), which their READMEs say |
| The same release | `nadir-cook_<version>_<arch>.deb` and `nadir-sandbox_<version>_<arch>.deb` for `amd64` and `arm64` (the file named for the release's version, as GitHub turns Debian's `~` into `.` in an asset's name); the apt repository's `Packages`, `Packages.gz`, `Release`, `InRelease`, `Release.gpg` and `nadir-archive-keyring.gpg`; and `SHA256SUMS` of every file |
| ghcr.io | `ghcr.io/jabbott-iii/nadir-cook:<version>`, and `:latest` unless the version is a pre-release, for `linux/amd64` and `linux/arm64`: the cook linked statically, `FROM scratch` (`tools/cook/Dockerfile`) |
| crates.io | 13 crates at `<version>`: `nadir_engine`, the facade (`nadir` in code; ADR-0039); `nadir-cook`; and `nadir-core`, `-sys`, `-platform`, `-gfx`, `-codec`, `-ecs`, `-input`, `-render`, `-assets`, `-collision` and `-scene`, each with copies of `LICENSE` and `NOTICE` (ADR-0037). The sandbox, `xtask`, `nadir-bench`, `nadir-audio` and `nadir-debug` have `publish = false` |

A version with a `-`, such as `0.2.0-rc.1`, is a pre-release: GitHub does
not mark it latest, so apt stays on the last full release, and the image
gets no `latest`. Debian writes it `0.2.0~rc.1`, which sorts before
`0.2.0`.

Installing them:

```bash
# apt, on Debian 12+ or Ubuntu 22.04+ (glibc 2.34), amd64 or arm64
curl -fsSL https://github.com/jabbott-iii/Nadir/releases/latest/download/nadir-archive-keyring.gpg \
  | sudo tee /usr/share/keyrings/nadir-archive-keyring.gpg > /dev/null
echo "deb [signed-by=/usr/share/keyrings/nadir-archive-keyring.gpg] https://github.com/jabbott-iii/Nadir/releases/latest/download ./" \
  | sudo tee /etc/apt/sources.list.d/nadir.list
sudo apt update && sudo apt install nadir-cook nadir-sandbox

# The image: cook ./assets into ./cooked, as the user who owns them
docker run --rm --user "$(id -u):$(id -g)" --volume "$PWD:/work" \
  ghcr.io/jabbott-iii/nadir-cook assets cooked

# crates.io: the engine in Cargo.toml as `nadir_engine = "0.2"`, used as
# `nadir::…`; and the cook as a command
cargo install nadir-cook
```

The keyring comes from the release it signs, so the first download is
trusted on first use: compare its fingerprint
(`gpg --show-keys nadir-archive-keyring.gpg`) with this one. apt then
refuses an index the key did not sign.

- **The apt key's fingerprint:** not made yet ("Setting it up, once",
  step 3, records it here).
apt reads only the newest release, and an `apt update` while a release
publishes can read two releases' files and fail; it passes when run again.

### Making a release

1. Raise `[workspace.package]`'s `version`, and each `version` in
   `[workspace.dependencies]`, in the root `Cargo.toml`; run
   `cargo xtask ci`.
2. Merge it to `main`, and wait for CI to pass on that commit.
3. Check out that commit, then tag it and push the tag:

   ```bash
   make release VERSION=v0.2.0
   ```

   The root `Makefile` first checks what the gate will, as the tag
   ruleset keeps a pushed tag from being moved or deleted: that `VERSION`
   is `v` and HEAD's version, and that HEAD is on the remote's `main`.
   `DRY_RUN=1` runs only the checks and prints the tag and the push;
   `REMOTE=` names a remote other than `origin`. Without `make`, it is:

   ```bash
   git tag -a v0.2.0 -m "Nadir 0.2.0"
   git push origin v0.2.0
   ```

One release runs at a time. GitHub keeps only the newest run that waits
for it, so push the next tag once the release before it has finished.

### The workflow

1. **`gate`** stops the run unless:
   - the tag is `v` plus `[workspace.package]`'s version;
   - its commit is on `main`;
   - and CI has a successful push run of that commit.

   Push the commit, wait for CI, then push the tag. A tag pushed too soon
   fails here: re-run the workflow once CI passes.
2. **`build`**, one job per target, each on a runner of that system and
   processor (`ubuntu-24.04`, `ubuntu-24.04-arm`, `windows-2025`,
   `windows-11-arm`, `macos-15-intel`, `macos-15`):
   - `cargo xtask dist` (`xtask/src/dist.rs`):
     - cooks `assets/`, and builds the cook and the sandbox in release,
       `--locked`;
     - lays them out with the cooked atlases, prefabs and levels in one
       pack, `assets/game.kpak` (ADR-0041), `README.txt`, `LICENSE` and
       `NOTICE`;
     - runs them: `nadir-cook --version` must print the version, the
       laid-out cook must cook `assets/` into the same pack byte for byte,
       and the sandbox must refuse an unknown scene,
       which it does
       before it opens a window;
     - packs the archive and, on Linux, the two `.deb`s, whose
       dependencies come from `dpkg-shlibdeps` and, for the sandbox, the
       libraries it loads at run time (`libx11-6`, `libegl1`,
       `libopengl0`).
   - On Linux, the sandbox from its archive draws 10 seconds of the field
     offscreen (`--capture`) on llvmpipe.
   - On the two Linux runners, the image's cook: `nadir-cook` built for
     `<arch>-unknown-linux-musl`, linked statically, and run.
3. **`crates`**, when every build passed, in the `crates-io` environment:
   `cargo publish --workspace --locked` with `CARGO_REGISTRY_TOKEN`, which
   packages and builds every crate from its package before it uploads any,
   then publishes them in the order of their dependencies. It goes first
   because crates.io refuses what no check here sees (a keyword too long,
   say), and a refusal then stops the release with nothing else out. A
   crate whose version crates.io has already is left out, so a failed
   attempt can run again. crates.io takes 5 new crates at once, then one
   each 10 minutes: an attempt it stops with `status 429` waits 10 minutes
   and tries again, up to 15 times. The first release, of 13 new crates,
   takes about 80 minutes there; later ones publish new versions, which
   crates.io takes 30 at a time.
4. When every crate is on crates.io, two jobs at once:
   - **`release`**, in the `release` environment, with `contents: write`:
     `.github/release/apt-repo.sh` makes and signs the apt repository
     from the `.deb`s with `APT_SIGNING_KEY`, `SHA256SUMS` sums every
     file, and `gh` makes the release as a draft with all of them, then
     publishes it, so that the newest release's files are never half
     there. A draft from a failed attempt is filled again; a published
     release is never changed.
   - **`image`**, with `packages: write`: logs in to ghcr.io with the
     job's token, makes a `docker-container` builder (the daemon's own
     makes one platform at a time), builds both platforms with
     `docker buildx` from the two static cooks (nothing runs, so no
     emulator), pushes them with no provenance attestation, and runs the
     pushed image's `--version`.

Each build job also checks that its runner's host is the target it is
for, and runs `cargo --locked xtask dist`, so a Cargo.lock left behind by
a version's raise fails the build rather than being rewritten.

Re-run the failed jobs once their cause is fixed. A published release, an
image tag that was pushed, or a crate version that crates.io has, is not
taken back: a broken release is mended by the next version.

### Setting it up, once

A maintainer does this in the repository's and the registries' settings.
The repository holds no secret, and the workflow names only these:

| Secret | Environment | What it is |
| :- | :- | :- |
| `APT_SIGNING_KEY` | `release` | The apt repository's signing key: an ASCII-armoured OpenPGP secret key with no passphrase, made for this repository alone |
| `CARGO_REGISTRY_TOKEN` | `crates-io` | A crates.io API token with the scopes `publish-new` and `publish-update` |

1. **Environments** (Settings → Environments): make `release` and
   `crates-io`. Under "Deployment branches and tags", allow only tags
   that match `v*`. A required reviewer on `crates-io` makes the whole
   release wait for approval before anything is published, as the
   GitHub release and the image wait for the crates; one on `release`
   holds the GitHub release alone.
2. **Who may tag** (Settings → Rules → Rulesets): a tag ruleset for
   `v*` that lets only the maintainers create tags, and blocks updating
   and deleting them. The gate checks a tag's version, `main` and CI, but
   it is a step of the tagged commit's own workflow: anyone who can push
   a `v*` tag can reach the environments' secrets.
3. **The apt key**, on a trusted machine:

   ```bash
   export GNUPGHOME="$(mktemp -d)"
   gpg --batch --passphrase '' --quick-gen-key \
     'Nadir apt repository <maintainer-address>' ed25519 sign never
   gpg --armor --export-secret-keys > apt-signing-key.asc
   gpg --fingerprint
   ```

   Store `apt-signing-key.asc` as `APT_SIGNING_KEY` in `release`, keep it
   and the revocation certificate in `$GNUPGHOME/openpgp-revocs.d` off
   the machine, and record the fingerprint under "Installing them"
   above. A new key needs a new
   keyring on every user's machine, so it is changed only if the old one
   is lost or exposed.
4. **The crates.io token:** crates.io → Account Settings → API Tokens,
   with `publish-new` and `publish-update`, and the crates limited to
   `nadir-*` and `nadir_engine` (the pattern `nadir-*` may not match the
   underscore of `nadir_engine`, so it has a line of its own; ADR-0039). Store it as `CARGO_REGISTRY_TOKEN` in
   `crates-io`. Once all 13 crates exist,
   crates.io's trusted publishing can replace it (ADR-0036).
5. **The image:** after the first release, make the package `nadir-cook`
   public (the account's Packages → `nadir-cook` → Package settings →
   Change visibility). Its `org.opencontainers.image.source` label links
   it to the repository.

## Game release pipeline (CI, on tag `v*`, M11.2)

The game's packages, signed and notarized, join `release.yml` with
M11.2. Until then the workflow publishes the engine's releases above.

1. `cargo xtask ci` on all three OSes.
2. `cargo xtask cook --release` to produce the deterministic `game.pak`.
   Record its content hash.
3. Run `xtask package` per OS, with signing and notarization.
4. Smoke-test each package on a clean runner. Launch with
   `--smoke-test`: the game boots, loads the first level, simulates 600 ticks
   headless-rendered, and exits 0.
5. Upload artifacts and symbols to the release storage.
6. Upload to stores in a separate, manually approved job (GitHub
   environment protection):
   - Steam via `steamcmd`, `run_app_build` with an app build VDF
   - itch.io via `butler push`

**Secrets:**

- Signing certificates, the Apple API key and store credentials live only in
  CI environment secrets, scoped to the release environment.
- The repo contains no secret material or paths to it.
- Rotation and ownership of each secret are documented in the team's
  password manager, not in this repo.

## Crash reporting

- **Panic hook** (all OSes; installed first thing in `main` by `App::run`):
  - Writes `crashes/<utc-timestamp>.txt` containing the panic message,
    `std::backtrace::Backtrace::force_capture()`, the build version and hash,
    OS version, GPU vendor/renderer string, and the last 200 log lines from
    the in-memory ring buffer.
  - Then shows a native message box (`MessageBoxW` / `NSAlert` / stderr on
    Linux) pointing to the file.
- **Native crashes:**
  - Windows: `SetUnhandledExceptionFilter` → `MiniDumpWriteDump` (from
    `dbghelp.dll`) into `crashes/<ts>.dmp`.
  - macOS: rely on the system crash reporter (`~/Library/Logs/DiagnosticReports/*.ips`).
  - Linux: rely on core dumps or `systemd-coredump`.
  - Document for players where to find these files.
- **Upload:**
  - There is no automatic upload.
  - On next launch after a crash, the game offers to open the crash folder.
  - An opt-in uploader is backlog, and would require a privacy review: no
    personal data, no file paths containing user names.
- **Symbolication:** we archive `.pdb`/`.dSYM`/`.dwp` per build hash.
  `xtask symbolicate <report>` maps addresses using the archived files and
  external tools: `llvm-symbolizer` on Linux, `atos` on macOS, WinDbg/`cdb`
  for Windows minidumps. Verify each tool's availability when the task is
  implemented.

## Release checklist (manual items)

- [ ] Golden tests run locally on Windows and macOS hardware (if CI cannot) — attach results.
- [ ] 10-minute audio soak with no underruns on each OS.
- [ ] Min-spec performance pass: 10k-sprite scene ≥ 60 fps; p99 frame ≤ 20 ms.
- [ ] Gamepad smoke test: Xbox, DualSense, Switch Pro on each OS (XInput-only limits on Windows noted).
- [ ] Save compatibility: load saves from the previous release.
- [ ] Notarization staple verified: `spctl -a -vv Game.app` reports "accepted".
- [ ] Windows SmartScreen: signed binary verified with `signtool verify /pa`.
- [ ] Linux: runs on the oldest supported distro and under XWayland on a Wayland session.
