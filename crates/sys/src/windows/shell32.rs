//! The shell: the user's known folders (`shell32.dll`).
//!
//! Sources: `shlobj_core.h` and `KnownFolders.h` of the Windows SDK.

// The functions, parameters and constants keep the names their headers give
// them (`FOLDERID_LocalAppData`), so that each one can be compared with its
// header.
#![allow(non_snake_case, non_upper_case_globals)]

use super::types::{GUID, HANDLE, HRESULT, PWSTR};

/// The folder for data that follows the user from machine to machine:
/// `%APPDATA%`, by default `C:\Users\<user>\AppData\Roaming`.
/// `KnownFolders.h`: `DEFINE_KNOWN_FOLDER(FOLDERID_RoamingAppData,
/// 0x3EB685DB, 0x65F9, 0x4CF6, 0xA0, 0x3A, 0xE3, 0xEF, 0x65, 0x72, 0x9F,
/// 0x3D);`
pub const FOLDERID_RoamingAppData: GUID = GUID {
    Data1: 0x3EB6_85DB,
    Data2: 0x65F9,
    Data3: 0x4CF6,
    Data4: [0xA0, 0x3A, 0xE3, 0xEF, 0x65, 0x72, 0x9F, 0x3D],
};

/// The folder for data that stays on this machine: `%LOCALAPPDATA%`, by
/// default `C:\Users\<user>\AppData\Local`.
/// `KnownFolders.h`: `DEFINE_KNOWN_FOLDER(FOLDERID_LocalAppData, 0xF1B32785,
/// 0x6FBA, 0x4FCF, 0x9D, 0x55, 0x7B, 0x8E, 0x7F, 0x15, 0x70, 0x91);`
pub const FOLDERID_LocalAppData: GUID = GUID {
    Data1: 0xF1B3_2785,
    Data2: 0x6FBA,
    Data3: 0x4FCF,
    Data4: [0x9D, 0x55, 0x7B, 0x8E, 0x7F, 0x15, 0x70, 0x91],
};

/// For [`SHGetKnownFolderPath`]: no special behavior.
/// `shlobj_core.h`: `KF_FLAG_DEFAULT = 0x00000000,` (in `enum
/// KNOWN_FOLDER_FLAG`)
pub const KF_FLAG_DEFAULT: u32 = 0x0000_0000;

#[link(name = "shell32")]
unsafe extern "system" {
    /// Finds the path of a known folder. On success, `*ppszPath` is the
    /// path, which ends with a NUL and has no backslash at the end. Returns
    /// a negative [`HRESULT`] on failure.
    ///
    /// The string is the caller's, success or failure: free it with
    /// `CoTaskMemFree`. It may be null after a failure.
    /// `shlobj_core.h`: `HRESULT SHGetKnownFolderPath(REFKNOWNFOLDERID rfid,
    /// DWORD dwFlags, HANDLE hToken, PWSTR *ppszPath)`
    ///
    /// # Safety
    ///
    /// `rfid` points to a [`GUID`], `hToken` is null (the current user) or a
    /// token handle, and `ppszPath` is valid for writes.
    pub fn SHGetKnownFolderPath(
        rfid: *const GUID,
        dwFlags: u32,
        hToken: HANDLE,
        ppszPath: *mut PWSTR,
    ) -> HRESULT;
}
