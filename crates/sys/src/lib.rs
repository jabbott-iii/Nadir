// Copyright 2026 Joseph Anthony Abbott III
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Hand-written raw FFI declarations for the Nadir engine: Win32, Cocoa/objc,
//! X11, EGL, GL and evdev so far. ALSA, CoreAudio and WASAPI come with the
//! audio (roadmap M6), and XInput with the gamepads (M8).
//!
//! This crate declares and does nothing else: types, constants, `extern`
//! blocks and symbol tables, each with the header it comes from, and layout
//! assertions. `nadir-platform` wraps them in a safe API.

// One of the three crates allowed to contain `unsafe` (AGENTS.md, rule 3). The
// workspace denies `unsafe_code` everywhere else.
#![allow(unsafe_code)]

pub mod dynlib;
pub mod gl;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "windows")]
pub mod windows;
