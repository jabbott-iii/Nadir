//! Declarations for Windows: Win32, linked from the system's DLLs.
//!
//! Every function here is in a DLL that Windows 10 has, and is linked at
//! build time through the Windows SDK's import libraries. The DPI functions
//! need Windows 10 1703, below the engine's floor of 1809.

pub mod gdi32;
pub mod kernel32;
pub mod ole32;
pub mod shell32;
pub mod types;
pub mod user32;

#[cfg(test)]
mod tests {
    use core::ptr;

    use super::gdi32::{BLACK_BRUSH, GetStockObject};
    use super::kernel32::GetModuleHandleW;
    use super::ole32::CoTaskMemFree;
    use super::shell32::{
        FOLDERID_LocalAppData, FOLDERID_RoamingAppData, KF_FLAG_DEFAULT, SHGetKnownFolderPath,
    };
    use super::types::{GUID, PWSTR, S_OK};
    use super::user32::{IDC_ARROW, LoadCursorW};

    /// Asks the shell for a known folder, and returns its path.
    fn known_folder(id: &GUID) -> String {
        let mut path: PWSTR = ptr::null_mut();
        // SAFETY: `id` is a GUID, a null token means the current user, and
        // `path` is valid for writes.
        let result =
            unsafe { SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, ptr::null_mut(), &raw mut path) };
        assert_eq!(result, S_OK);
        let mut units = Vec::new();
        for index in 0.. {
            // SAFETY: on success the path ends with a NUL, and this stops at
            // it, so every unit read is inside the string.
            let unit = unsafe { path.wrapping_add(index).read() };
            if unit == 0 {
                break;
            }
            units.push(unit);
        }
        // SAFETY: the shell allocated the string for us, with COM's
        // allocator, and it is not used after this.
        unsafe { CoTaskMemFree(path.cast()) };
        String::from_utf16_lossy(&units)
    }

    #[test]
    fn the_known_folder_ids_name_the_app_data_folders() {
        // What the folders are called is the same in every language of
        // Windows; only Explorer shows translated names.
        let roaming = known_folder(&FOLDERID_RoamingAppData);
        assert!(roaming.ends_with(r"\AppData\Roaming"), "{roaming}");
        let local = known_folder(&FOLDERID_LocalAppData);
        assert!(local.ends_with(r"\AppData\Local"), "{local}");
    }

    #[test]
    fn the_system_has_the_program_the_arrow_and_the_black_brush() {
        // SAFETY: a null name asks for the program itself.
        let program = unsafe { GetModuleHandleW(ptr::null()) };
        assert!(!program.is_null());
        // SAFETY: `IDC_ARROW` is a standard pointer shape, which needs no
        // module.
        let arrow = unsafe { LoadCursorW(ptr::null_mut(), IDC_ARROW) };
        assert!(!arrow.is_null());
        assert!(!GetStockObject(BLACK_BRUSH).is_null());
    }
}
