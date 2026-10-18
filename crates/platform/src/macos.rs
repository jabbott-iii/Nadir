//! The macOS backend: an AppKit window, the user's folders, and alerts.

mod alert;
mod input;
mod keymap;
mod paths;
mod runtime;
mod window;

pub(crate) use alert::alert;
pub(crate) use paths::user_dirs;
pub(crate) use window::Backend;
