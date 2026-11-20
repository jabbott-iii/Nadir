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

//! What system the game runs on, for crash reports and logs.

#[cfg(target_os = "linux")]
use crate::linux::os_description as system_description;

/// Describes the operating system the game runs on and the processor's
/// architecture, in one line, for crash reports and logs.
///
/// - On Linux: the distribution's name from `os-release`, the kernel's
///   version and the architecture, such as
///   `Ubuntu 24.04.1 LTS (Linux 6.8.0-49-generic, x86_64)`. A part that
///   cannot be read is left out, or said to be unknown.
/// - On Windows and macOS: the system's name and the architecture only, as
///   `Windows (x86_64)` or `macOS (aarch64)`, until roadmap M1.5 and M1.6
///   read their versions.
///
/// On Linux it reads two small files, so call it once, not every frame.
pub fn os_description() -> String {
    system_description()
}

/// The system's name and the architecture: what every system can say.
#[cfg(not(target_os = "linux"))]
fn system_description() -> String {
    use std::env::consts::{ARCH, OS};

    format!("{} ({ARCH})", system_name(OS))
}

/// The name of `os`, one of [`std::env::consts::OS`]'s values, as people
/// write it.
#[cfg(any(test, not(target_os = "linux")))]
fn system_name(os: &str) -> &str {
    match os {
        "linux" => "Linux",
        "macos" => "macOS",
        "windows" => "Windows",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use std::env::consts::{ARCH, OS};

    use super::*;

    #[test]
    fn systems_are_named_as_people_write_them() {
        assert_eq!(system_name("linux"), "Linux");
        assert_eq!(system_name("macos"), "macOS");
        assert_eq!(system_name("windows"), "Windows");
        assert_eq!(system_name("freebsd"), "freebsd");
    }

    #[test]
    fn the_description_names_the_system_and_the_architecture() {
        let description = os_description();
        assert!(description.contains(system_name(OS)), "{description}");
        assert!(description.ends_with(&format!("{ARCH})")), "{description}");
        assert!(!description.contains('\n'), "{description}");
    }
}
