---
paths:
  - "**/Cargo.toml"
  - "Cargo.lock"
  - ".cargo/**"
  - "xtask/**"
  - "rust-toolchain.toml"
---

# Rules for manifests and dependencies

- **No third-party crates.** Do not add any non-workspace crate to
  `[dependencies]`, `[dev-dependencies]` or `[build-dependencies]` in any
  manifest. The approved set is listed in `docs/02-dependency-policy.md`,
  and it is currently empty.
- **If you think a crate is needed, stop.**
  1. Run `/evaluate-dependency <crate> <need>`.
  2. Present the evaluation to the user.
  3. Change nothing until the user approves.
- **Workspace dependencies must follow the matrix** in
  `docs/01-architecture.md` → "Dependency matrix". If a task seems to need a
  new edge, stop and ask. Do not edit the matrix to make code compile.
- **Lints:** every member uses `[lints] workspace = true`. Never weaken
  `[workspace.lints]`, `clippy.toml` or `unsafe_code`. Never add
  `#![allow(...)]` to silence a lint without a comment explaining why the
  lint is wrong in that spot.
- **Toolchain:** don't change the pinned toolchain in `rust-toolchain.toml`
  as a side effect of another task. Toolchain bumps are their own task.
- **Lockfile:** `Cargo.lock` must stay free of `source = ` entries. Check
  with `cargo xtask deps` (or `grep -c 'source = ' Cargo.lock` before M0.6
  exists).
- **Build scripts:** `build.rs` may only emit `cargo:` directives (link
  args, rerun-if-changed). No network access, no running external tools.
  Windows resource compilation happens in `xtask package`, not `build.rs`.
- **xtask:**
  - must never upload, publish, sign with real credentials, or read secrets
  - must not call the network
