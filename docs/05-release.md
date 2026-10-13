# 05 — Build, release, operate

## Cargo profiles

```toml
[profile.dev]
opt-level = 1                  # debuggable but not glacial

# Hot, rarely-stepped-through crates can be optimized harder in dev.
# Decide per crate in M0 by measuring sandbox frame time:
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

`LICENSES.txt` lists our own license and the licenses of any approved
dependencies. With zero dependencies, that's just ours. Fonts and other
content have their own entries.

## Release pipeline (CI, on tag `v*`)

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
