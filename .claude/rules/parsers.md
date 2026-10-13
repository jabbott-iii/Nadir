---
paths:
  - "crates/codec/**"
  - "crates/assets/**"
  - "crates/scene/**"
  - "tools/cook/**"
  - "fuzz/**"
---

# Rules for parsers, decoders and on-disk formats

Every byte we read from disk may be malformed or hostile: saves, mods,
downloaded content, and corrupted files.

- **Never panic on input.**
  - No indexing that can go out of bounds on input-derived offsets.
  - No `unwrap`/`expect` on input-derived values.
  - No unchecked arithmetic on input lengths: use `checked_add`/`checked_mul`
    and return an error on overflow.
- **Check before allocating.** Compare every size against the `Limits`
  struct, and the claimed size against the remaining input, before calling
  `Vec::with_capacity` or `resize`.
- **Bound recursion** (JSON, KBJ, prefab `extends`) by an explicit depth
  limit. Exceeding it is an error, never a stack overflow.
- **Errors carry context:** the format, the byte offset (or line and column
  for text), and what was expected.
- **Every new parser:**
  1. registers an `xtask fuzz-smoke` target with a seed corpus in
     `fuzz/corpus/<target>/`
  2. has hand-made test vectors committed beside its tests
  3. documents its supported subset in
     `docs/subsystems/assets-and-codecs.md`
- **Format changes:** any change to an on-disk format (`.kpak`, `.klvl`,
  `.ksav`, KBJ, KLZ, settings) bumps its version. It needs a migration, or an
  explicit "not supported" error for old versions, and a fixture test.
- **Cook output must be deterministic:** sorted iteration, no timestamps, the
  deterministic hasher only. Two cooks of the same input produce
  byte-identical output.
- **Paths:** normalize asset paths to lowercase with `/` separators. Reject
  `..`, absolute paths, and case-colliding names at cook time.
