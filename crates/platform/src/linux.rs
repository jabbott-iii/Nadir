//! The Linux backend: X11 through Xlib, which is loaded when the window is
//! opened. A Wayland session runs it through XWayland.

mod keymap;
mod library;
mod x11;
mod xresources;

pub(crate) use x11::Backend;
