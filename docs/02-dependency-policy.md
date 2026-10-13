# 02 — Dependency policy

## The rule

> **Build it ourselves unless building it is unreasonable.** A third-party
> crate is used only when it passes the exception test below, the user has
> approved it, and it is recorded in the register in this document.

The current approved set of third-party crates is **empty**. `Cargo.lock` must
contain only workspace packages. `cargo xtask deps` fails otherwise.

## What counts as a dependency

| Counts (needs approval) | Does not count |
| :- | :- |
| Any crate from crates.io or git in any `[dependencies]`, `[dev-dependencies]` or `[build-dependencies]` table | The Rust toolchain and `std` |
| Vendored or copied third-party source code, including "just one file" | OS system libraries and frameworks we call through FFI (listed below) |
| A build script that downloads anything or runs third-party code | OS and SDK tools invoked by `xtask` packaging (`signtool`, `rc`, `codesign`, `notarytool`, `lipo`, `iconutil`, `hdiutil`, `appimagetool`) |
| A git submodule containing code | Content tools whose output files we parse (LDtk, Aseprite) |
| A `cargo install`-ed tool that CI depends on | GitHub Actions used by CI. Pin by commit SHA; see `docs/04-quality.md` |

Reading a public file format specification (PNG, QOI, QOA, WAV, BMFont, LDtk
JSON) and implementing it ourselves is **not** a dependency. That is the
intended path.

### OS libraries we rely on (not dependencies, but listed for audit)

| OS | Libraries / frameworks | How |
| :- | :- | :- |
| Windows | `kernel32`, `user32`, `gdi32`, `opengl32`, `winmm`, `ole32`, `shell32`, `dbghelp`, `avrt`, `xinput1_4` (fallback `xinput9_1_0`) | Linked import libraries from the MSVC toolchain; XInput, avrt and dbghelp loaded with `LoadLibraryW` |
| macOS | `libobjc`, `Foundation`, `AppKit`, `QuartzCore`, `OpenGL`, `Metal`, `AudioToolbox`, `CoreAudio`, `CoreFoundation`, `GameController` | `#[link(name = "...", kind = "framework")]` in `nadir-sys` |
| Linux | `libc` (glibc), `libX11.so.6`, `libEGL.so.1`, `libOpenGL.so.0` (fallback for GL symbols), `libasound.so.2`; later `libXi.so.6` | glibc linked; everything else loaded with `dlopen`, so a missing library produces a clear error message instead of a loader failure |

## The exception test

Before proposing any crate, first try to **change the design so the need
disappears**:

- choose a simpler format (QOA instead of Vorbis)
- cut scope (bitmap fonts instead of TrueType)
- move the work offline (PNG decoding only in the cook tool)

If the need remains, a crate may be proposed only when at least one of these
is true:

1. **Disproportionate cost.** Building it is estimated above 4 engineer-weeks,
   the capability is not central to the engine, and no design change avoids
   it.
2. **Specialist correctness with severe failure modes.** The domain demands
   expert knowledge, bugs are hard to detect by testing, and the consequences
   are severe. Examples: cryptography, TLS. Nothing in the current scope
   needs these.
3. **Dev-only tooling with no reasonable equivalent.** It never ships in the
   game binary, and our own version would be far weaker. Example:
   coverage-guided fuzzing.

"It's popular" and "it would be faster to start" are not reasons.

## Exception process

1. Run `/evaluate-dependency` (or follow `.claude/skills/evaluate-dependency/`
   manually) and write the evaluation. It must cover:
   - the need, and the design changes considered
   - an estimate for building it ourselves
   - the candidate crate's license, maintainers and release cadence
   - its transitive dependency count and `unsafe` usage
   - its binary-size impact, and whether it ships in the game binary
2. The **user** approves or rejects it. Agents never approve dependencies.
3. If approved:
   - Add a row to the register below with status `Approved` and an ADR
     reference.
   - Add the exact pin to `xtask/deps-allowlist.txt` (`name version ADR-NNNN`).
   - Use an exact version requirement (`= x.y.z`).
   - Wrap the crate behind a single module or crate boundary of ours, so it
     can be replaced without touching callers.
4. Review approved dependencies on every upgrade. Upgrades are their own
   change, never bundled with feature work.

## Build-vs-buy register

**Status** is one of:

- **Build:** we write it.
- **Proposed:** awaiting user decision.
- **Approved:** user said yes, ADR recorded.
- **Rejected.**

The **fallback** column is a pre-evaluated crate we would consider if the
**switch trigger** fires. It is not approved.

| # | Capability | Status | Our approach | Fallback (not approved) | Switch trigger |
| :- | :- | :- | :- | :- | :- |
| 1 | Win32 / COM bindings | Build | Hand-written in `nadir-sys::windows`, only what we call, with layout tests | `windows-sys` (bindings only) | More than about 1,500 declarations, or more than 2 layout bugs reach `main` in one milestone |
| 2 | Objective-C runtime glue | Build | Own `msg_send!` macro with exact fn-pointer casts, class builder, autorelease pool guard | `objc2` family | More than 2 ABI bugs in one milestone |
| 3 | X11 / EGL / ALSA bindings | Build | Hand-written, `dlopen`-loaded symbol tables | `x11-dl` | Same as #1 |
| 4 | Windowing and event loop | Build | `nadir-platform`, one module per OS | `winit` | Not expected |
| 5 | GPU abstraction | Build | Own RHI: OpenGL 3.3 core everywhere, Metal on macOS from M10 (ADR-0004) | `wgpu` | A need for compute or features GL 3.3 lacks; first consider an own D3D11 or Vulkan backend |
| 6 | Shader cross-compilation | Build | Hand-written GLSL 330 and MSL per shader, parity tests (ADR-0005) | `naga` | More than about 20 shader programs, or repeated GLSL/MSL divergence bugs |
| 7 | Math (vectors, matrices, rects) | Build | `nadir-core::math` | `glam` | Not expected |
| 8 | ECS | Build | Sparse-set ECS, single-threaded schedule (ADR-0006) | `hecs` | ECS overhead above 25% of tick time at budget load after optimization |
| 9 | Serialization and reflection | Build | `macro_rules!` reflection plus own binary and JSONC formats (ADR-0007) | `serde` + `serde_json` | Macro limitations block more than 3 needed component shapes |
| 10 | PNG decoding (cook tool only) | Build | Own inflate + PNG subset in `nadir-codec` behind feature `png` | `png` (cook only) | Required PNG features beyond the subset cost more than 1 week |
| 11 | Runtime texture format | Build | QOI (own encoder and decoder) | — | — |
| 12 | Audio codec | Build | QOA (own encoder and decoder) and PCM WAV (ADR-0008) | `lewton` (Vorbis decode) | QOA music pushes the download over budget |
| 13 | Audio device output | Build | WASAPI, AudioUnit, ALSA in `nadir-platform` | `cpal` | Device-switching/hotplug defects unresolved across 2 releases |
| 14 | Mixer | Build | `nadir-audio` | `kira` | Not expected |
| 15 | Gamepads | Build | XInput, GameController.framework, evdev | `gilrs` | Controller-support reports exceed backlog capacity |
| 16 | Text rendering | Build | Bitmap fonts (BMFont text format); own TrueType rasterizer in cook is backlog (ADR-0010) | `fontdue` (cook only) | Localization into scripts needing TrueType before the backlog item lands |
| 17 | Debug / immediate-mode UI | Build | `nadir-debug` | `egui` | Not expected |
| 18 | Profiling | Build | Scoped timers plus Chrome Trace Event JSON export | Tracy client | Not expected |
| 19 | Logging | Build | `nadir-core::log` | `log` / `tracing` | Not expected |
| 20 | Pack-file compression | Build | Own LZ77 block format ("KLZ") | `lz4_flex` | Not expected |
| 21 | JSON / JSONC | Build | Own parser and writer in `nadir-codec::json` | `serde_json` | Not expected |
| 22 | Hashing | Build | FNV-1a 64 (asset keys), own fast non-cryptographic hasher (maps), CRC32 (integrity) | — | A cryptographic need appears (e.g. signed mods): that triggers criterion 2 |
| 23 | RNG | Build | PCG32, seedable, deterministic | `rand` | Not expected |
| 24 | Thread pool and channels | Build | `std::thread`, `std::sync::mpsc`, `Mutex`/`Condvar` | `rayon`, `crossbeam` | Not expected |
| 25 | File watching (dev) | Build | mtime polling thread | `notify` | Polling cost becomes noticeable on very large asset trees |
| 26 | Property testing | Build | Seeded generator harness in `nadir-core::testing` | `proptest` | Not expected |
| 27 | Benchmarking | Build | `nadir-bench` harness: warm-up, N samples, median/p95, JSON baseline | `criterion` | Not expected |
| 28 | Fuzzing (stable, CI) | Build | Mutation "smoke" fuzzer in `xtask` | — | — |
| 29 | Coverage-guided fuzzing (optional, nightly) | **Proposed** | `cargo fuzz` with `libfuzzer-sys` in an excluded `fuzz/` workspace, dev-only, never linked into the game | — | User decision in M11 (criterion 3) |
| 30 | Native crash dumps | Build | `SetUnhandledExceptionFilter` + `MiniDumpWriteDump` (Windows); OS crash reporters on macOS and Linux | `sentry` / `minidumper` | Not expected |
| 31 | CLI argument parsing | Build | Tiny parser in `nadir-core::cli` | `clap` | Not expected |
| 32 | Timestamps | Build | UTC civil date from `SystemTime` (days-to-civil algorithm) | `chrono`, `time` | Not expected |
| 33 | Error plumbing | Build | Per-crate error enums | `anyhow`, `thiserror` | Not expected |
| 34 | Byte casting (`Pod`) | Build | `unsafe trait Pod` in `nadir-gfx` with size asserts | `bytemuck` | Not expected |

## Enforcement

- **`cargo xtask deps`** reads `cargo metadata --format-version 1` (parsed
  with our own JSON parser). It fails if any package has a non-null `source`
  and does not appear in `xtask/deps-allowlist.txt`.
- **`cargo xtask layers`** checks the workspace dependency graph against the
  matrix in `docs/01-architecture.md`.
- **Path-scoped rule** `.claude/rules/dependencies.md` loads whenever an agent
  opens a Cargo manifest.
- **Claude Code permissions** ask before `cargo add`, `cargo update` and
  `cargo install`.
