//! OS behavior for the Nadir engine: windows, the event loop, input devices,
//! GL/Metal surfaces, audio device output, paths and dynamic loading.

// One of the three crates allowed to contain `unsafe` (AGENTS.md, rule 3). The
// workspace denies `unsafe_code` everywhere else.
#![allow(unsafe_code)]
