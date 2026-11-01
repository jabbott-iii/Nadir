<!--
Thank you for contributing to Nadir. Read CONTRIBUTING.md before opening this
pull request: the project rules in AGENTS.md apply to every change.
A pull request needs an issue on which a maintainer confirmed the change
(CONTRIBUTING.md, "Contribution workflow"), unless it fixes a typo or a broken link.
Replace the comments below, and remove the sections that do not apply.
-->

## Linked issue and roadmap task

Closes #

- Roadmap task: <!-- such as M5.9, or "none" -->
- [ ] A maintainer confirmed the change on the linked issue.

## Summary

<!-- What changed and why. Keep the pull request to the one issue or task. -->

## Type of change

- [ ] Bug fix
- [ ] Roadmap task (every acceptance criterion in `docs/roadmap.md` is met)
- [ ] New feature or change outside the roadmap (recorded in `docs/decisions.md`)
- [ ] Breaking change: a public API, an on-disk format, the dependency matrix or a CLI
- [ ] Performance
- [ ] Documentation
- [ ] Build, CI or `xtask`
- [ ] Demo page (`site/`, `docs/features.jsonc`, `.github/workflows/pages.yml`)
- [ ] Tests or refactor only, with no change in behavior

## Validation

<!-- Tick only what you ran, and paste the output below. Say which checks
you could not run, and why: a check that needs Windows or macOS, a GPU,
or GStreamer, for example. -->

- [ ] `cargo xtask ci` (fmt, clippy `-D warnings`, `cook`, tests, `deps`, `layers`, `unsafe-audit`, `workflows`, `release-no-dev`, `features`)
- [ ] `cargo xtask golden`, for changes to `gfx`, `render`, `platform` or the scenes (Linux on llvmpipe; see `docs/04-quality.md`)
- [ ] `cargo xtask fuzz-smoke`, for changes to a parser: `codec`, `assets`, `scene` or the cook
- [ ] `cargo xtask bench <filter>`, for performance work (paste the numbers before and after)
- [ ] `cargo xtask cook`, for changes to the cook or `assets/`
- [ ] `cargo xtask site`, for changes to the demo page (needs EGL and GStreamer)
- [ ] `cargo run -p sandbox`, checked by hand (describe what you saw below)

**Tested on:** <!-- OS and version (Linux X11 or XWayland, Windows, macOS), CPU, GPU and driver for rendering changes, and `rustc -V` -->

**Not run, and why:** <!-- or "none" -->

<details>
<summary>Command output</summary>

```text
paste output here
```

</details>

## Checklist

- [ ] No crate from outside the workspace is added, and `Cargo.lock` has no `source =` entries (`docs/02-dependency-policy.md`).
- [ ] Each crate depends only on the crates its row of the dependency matrix in `docs/01-architecture.md` allows.
- [ ] `unsafe` is only in `nadir-sys`, `nadir-platform` and `nadir-gfx`; each block has a `// SAFETY:` comment, and each `pub unsafe fn` a `# Safety` section.
- [ ] `#[cfg(target_os = ...)]` is only where `AGENTS.md` allows it.
- [ ] No `unwrap()` in library code, every `pub` item is documented, and no lint is weakened.
- [ ] Tests cover the change. No test is deleted or ignored, no tolerance is raised and no golden image is re-blessed to make a check pass, unless the summary says why.
- [ ] An on-disk format change bumps its version, with a migration or a clear error for old files, and a fixture test.
- [ ] No per-frame heap allocation is added to rendering, ECS iteration, audio callbacks or collision.
- [ ] Docs are updated where behavior or design changed: the subsystem spec, an ADR in `docs/decisions.md`, and for a roadmap task its tick and its feature in `docs/features.jsonc`.
- [ ] No secrets, signing material, tokens, build output (`target/`) or captures are included.
- [ ] If a coding agent wrote part of this change, I have reviewed all of it, and the agent followed `AGENTS.md`.

## Security and compatibility

<!--
Security: does this change how untrusted files are read (parsers, assets,
saves, the cook), `unsafe` code or OS bindings, CI workflows and their
permissions, or what the demo page publishes? Describe the effect.
Never describe an undisclosed vulnerability here: report it privately as
SECURITY.md describes.
Compatibility: does this change a public API, an on-disk format, a command's
arguments or the sandbox's controls? Write "None" if neither applies.
-->

## Screenshots or recordings

<!-- For a change to what the engine draws, the sandbox or the demo page:
before and after. Remove this section otherwise. -->

## Notes for reviewers

<!-- What to look at first, known limits, and follow-up work. -->
