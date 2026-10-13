//! Small render hardware interface (RHI) for the Nadir engine, with an
//! OpenGL 3.3 core backend and, from M10, a Metal backend.

// One of the three crates allowed to contain `unsafe` (AGENTS.md, rule 3). The
// workspace denies `unsafe_code` everywhere else.
#![allow(unsafe_code)]
