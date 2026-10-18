//! Hand-written raw FFI declarations for the Nadir engine: Win32, Cocoa/objc,
//! X11, EGL, GL, ALSA, CoreAudio, WASAPI, XInput and evdev.
//!
//! This crate declares and does nothing else: types, constants, `extern`
//! blocks and symbol tables, each with the header it comes from, and layout
//! assertions. `nadir-platform` wraps them in a safe API.

// One of the three crates allowed to contain `unsafe` (AGENTS.md, rule 3). The
// workspace denies `unsafe_code` everywhere else.
#![allow(unsafe_code)]

pub mod dynlib;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "windows")]
pub mod windows;
