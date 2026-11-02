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

//! The graphics device interface: the system's brushes (`gdi32.dll`).
//!
//! Source: `wingdi.h` of the Windows SDK.

// The functions keep the names their headers give them, so that each one can
// be compared with its header.
#![allow(non_snake_case)]

use super::types::HGDIOBJ;

/// A stock brush that paints black, for [`GetStockObject`].
/// `wingdi.h`: `#define BLACK_BRUSH 4`
pub const BLACK_BRUSH: i32 = 4;

#[link(name = "gdi32")]
unsafe extern "system" {
    /// Returns one of the system's stock pens, brushes or fonts, or null on
    /// failure. The system owns it: nobody frees it.
    /// `wingdi.h`: `HGDIOBJ GetStockObject(int i)`
    pub safe fn GetStockObject(i: i32) -> HGDIOBJ;
}
