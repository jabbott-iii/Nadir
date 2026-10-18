//! The Linux backend: X11 through Xlib, which is loaded when the window is
//! opened. A Wayland session runs it through XWayland.

mod egl;
mod keymap;
mod library;
mod x11;
mod xdg;
mod xresources;

pub(crate) use egl::Headless;
pub(crate) use x11::Backend;
pub(crate) use xdg::user_dirs;
