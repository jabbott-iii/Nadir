//! Hand-written raw FFI declarations for the Nadir engine: Win32, Cocoa/objc,
//! X11, EGL, GL, ALSA, CoreAudio, WASAPI, XInput and evdev.

// One of the three crates allowed to contain `unsafe` (AGENTS.md, rule 3). The
// workspace denies `unsafe_code` everywhere else.
#![allow(unsafe_code)]
