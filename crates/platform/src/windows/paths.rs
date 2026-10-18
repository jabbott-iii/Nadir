//! The user's folders, from the shell's known folders.

use core::{ptr, slice};
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use nadir_sys::windows::ole32::CoTaskMemFree;
use nadir_sys::windows::shell32::{
    FOLDERID_LocalAppData, FOLDERID_RoamingAppData, KF_FLAG_DEFAULT, SHGetKnownFolderPath,
};
use nadir_sys::windows::types::{GUID, PWSTR};

use crate::{PlatformError, UserDirs};

/// Finds the game's folders: settings in the roaming application data, and
/// data and logs in the local one, each in `<studio>\<game>`.
pub(crate) fn user_dirs(studio: &str, game: &str) -> Result<UserDirs, PlatformError> {
    let roaming = known_folder(&FOLDERID_RoamingAppData)?;
    let local = known_folder(&FOLDERID_LocalAppData)?;
    Ok(dirs_in(&roaming, &local, studio, game))
}

/// The game's folders, given the two application data folders.
fn dirs_in(roaming: &Path, local: &Path, studio: &str, game: &str) -> UserDirs {
    let data = local.join(studio).join(game);
    UserDirs {
        config: roaming.join(studio).join(game),
        logs: data.clone(),
        data,
    }
}

/// A string that COM's allocator holds for us, freed when this is dropped.
struct TaskString(PWSTR);

impl TaskString {
    /// The UTF-16 units before the NUL.
    ///
    /// # Safety
    ///
    /// The pointer is not null, and the string ends with a NUL.
    unsafe fn units(&self) -> &[u16] {
        let mut len = 0;
        // SAFETY: the caller promises a NUL at the end, and this stops there,
        // so every unit read is inside the string.
        while unsafe { self.0.wrapping_add(len).read() } != 0 {
            len += 1;
        }
        // SAFETY: the `len` units before the NUL are initialized, and live
        // as long as `self`.
        unsafe { slice::from_raw_parts(self.0, len) }
    }
}

impl Drop for TaskString {
    fn drop(&mut self) {
        // SAFETY: the pointer is null, which is allowed, or came from COM's
        // allocator, and nothing uses it after this.
        unsafe { CoTaskMemFree(self.0.cast()) };
    }
}

/// The path of a known folder.
fn known_folder(id: &GUID) -> Result<PathBuf, PlatformError> {
    let mut path: PWSTR = ptr::null_mut();
    // SAFETY: `id` is a GUID, a null token means the current user, and
    // `path` is valid for writes.
    let result =
        unsafe { SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, ptr::null_mut(), &raw mut path) };
    // The string is ours to free, whatever the result.
    let path = TaskString(path);
    if result < 0 || path.0.is_null() {
        return Err(PlatformError::CallFailed {
            call: "SHGetKnownFolderPath",
        });
    }
    // A path can hold UTF-16 that is not valid Unicode; `OsString` keeps it.
    // SAFETY: on success the string is not null, and ends with a NUL.
    let units = unsafe { path.units() };
    Ok(PathBuf::from(OsString::from_wide(units)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_roam_and_data_and_logs_stay_local() {
        let roaming = PathBuf::from(r"C:\Users\Ada\AppData\Roaming");
        let local = PathBuf::from(r"C:\Users\Ada\AppData\Local");
        let dirs = dirs_in(&roaming, &local, "Studio", "Game");
        assert_eq!(
            dirs.config,
            PathBuf::from(r"C:\Users\Ada\AppData\Roaming\Studio\Game")
        );
        assert_eq!(
            dirs.data,
            PathBuf::from(r"C:\Users\Ada\AppData\Local\Studio\Game")
        );
        assert_eq!(dirs.logs, dirs.data);
    }

    #[test]
    fn the_real_folders_are_absolute_and_end_with_the_names() {
        let dirs = user_dirs("Studio", "Game").expect("the shell knows the folders");
        for path in [&dirs.config, &dirs.data, &dirs.logs] {
            assert!(path.is_absolute(), "{}", path.display());
            assert!(path.ends_with(r"Studio\Game"), "{}", path.display());
        }
        assert!(
            dirs.config.parent().is_some_and(|studio| studio
                .parent()
                .is_some_and(|roaming| roaming.ends_with("Roaming"))),
            "{}",
            dirs.config.display()
        );
    }
}
