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

//! COM's memory allocator (`ole32.dll`).
//!
//! Source: `combaseapi.h` of the Windows SDK.

// The functions keep the names their headers give them, so that each one can
// be compared with its header.
#![allow(non_snake_case)]

use core::ffi::c_void;

#[link(name = "ole32")]
unsafe extern "system" {
    /// Frees memory that COM, or a function such as `SHGetKnownFolderPath`,
    /// allocated for the caller. A null pointer is allowed, and does nothing.
    /// `combaseapi.h`: `void CoTaskMemFree(LPVOID pv)`
    ///
    /// # Safety
    ///
    /// `pv` is null, or came from COM's allocator and is not freed yet.
    pub fn CoTaskMemFree(pv: *mut c_void);
}
