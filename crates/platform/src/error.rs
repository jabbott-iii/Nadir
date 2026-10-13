//! The error type for `nadir-platform`.

use std::fmt;

/// Errors produced by `nadir-platform`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlatformError {
    /// This operating system has no backend yet.
    Unsupported,
    /// There is no display server to connect to.
    NoDisplay {
        /// Whether a Wayland session is running. If so, it runs without
        /// XWayland.
        wayland: bool,
    },
    /// A system library could not be loaded.
    LibraryNotFound {
        /// The file name of the library, such as `libX11.so.6`.
        library: &'static str,
        /// What the system's loader said.
        reason: String,
    },
    /// A system library lacks functions that the engine calls.
    MissingSymbols {
        /// The file name of the library.
        library: &'static str,
        /// The name of every missing function.
        symbols: Vec<&'static str>,
    },
    /// The display server did not accept the connection.
    ConnectionFailed {
        /// The display that was tried, as `DISPLAY` names it.
        display: String,
    },
    /// A call to the operating system failed.
    CallFailed {
        /// The function that failed.
        call: &'static str,
    },
}

impl fmt::Display for PlatformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => f.write_str("this operating system has no window backend yet"),
            Self::NoDisplay { wayland: false } => {
                f.write_str("no X server to connect to: `DISPLAY` is not set")
            }
            Self::NoDisplay { wayland: true } => f.write_str(
                "no X server to connect to: `DISPLAY` is not set. This is a Wayland session, \
                 and the game needs XWayland in it. Turn XWayland on in the compositor's \
                 settings, then start the game again",
            ),
            Self::LibraryNotFound { library, reason } => {
                write!(f, "could not load {library}: {reason}")
            }
            Self::MissingSymbols { library, symbols } => write!(
                f,
                "{library} lacks functions that the engine calls: {}",
                symbols.join(", ")
            ),
            Self::ConnectionFailed { display } => {
                write!(f, "could not connect to the X server `{display}`")
            }
            Self::CallFailed { call } => write!(f, "{call} failed"),
        }
    }
}

impl std::error::Error for PlatformError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_message_says_what_went_wrong() {
        let cases = [
            (
                PlatformError::Unsupported,
                "this operating system has no window backend yet",
            ),
            (
                PlatformError::NoDisplay { wayland: false },
                "no X server to connect to: `DISPLAY` is not set",
            ),
            (
                PlatformError::LibraryNotFound {
                    library: "libX11.so.6",
                    reason: "cannot open shared object file".to_string(),
                },
                "could not load libX11.so.6: cannot open shared object file",
            ),
            (
                PlatformError::MissingSymbols {
                    library: "libX11.so.6",
                    symbols: vec!["XPending", "XNextEvent"],
                },
                "libX11.so.6 lacks functions that the engine calls: XPending, XNextEvent",
            ),
            (
                PlatformError::ConnectionFailed {
                    display: ":7".to_string(),
                },
                "could not connect to the X server `:7`",
            ),
            (
                PlatformError::CallFailed {
                    call: "XSetWMProtocols",
                },
                "XSetWMProtocols failed",
            ),
        ];
        for (error, message) in cases {
            assert_eq!(error.to_string(), message);
        }
    }

    #[test]
    fn a_wayland_session_without_xwayland_is_told_what_to_do() {
        let message = PlatformError::NoDisplay { wayland: true }.to_string();
        assert!(
            message.starts_with("no X server to connect to"),
            "{message}"
        );
        assert!(message.contains("Wayland session"), "{message}");
        assert!(message.contains("Turn XWayland on"), "{message}");
    }

    #[test]
    fn it_is_an_error() {
        let error: Box<dyn std::error::Error> = Box::new(PlatformError::Unsupported);
        assert!(error.source().is_none());
    }
}
