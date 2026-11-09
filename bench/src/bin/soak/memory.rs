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

//! The process's memory, as Linux's `/proc/self/status` gives it. It is read
//! as a file, with no `cfg`: where there is no such file, as on Windows and
//! macOS, there is no reading (ADR-0052).

use std::fs;

/// The file read.
const STATUS: &str = "/proc/self/status";

/// The process's memory in RAM, in bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Memory {
    /// All of it: its resident set (`VmRSS`), the libraries' pages and the
    /// program's included, which come and go with the system's page cache.
    pub(crate) resident: u64,
    /// The part that is the process's own (`RssAnon`): its heaps and
    /// stacks, which a leak grows.
    pub(crate) anonymous: u64,
    /// The most it has held at once since it started (`VmHWM`).
    pub(crate) peak: u64,
}

/// Reads the process's memory now: `None` where the system has no
/// `/proc/self/status`, or it lacks a field.
pub(crate) fn read() -> Option<Memory> {
    parse(&fs::read_to_string(STATUS).ok()?)
}

/// Reads `VmRSS`, `RssAnon` and `VmHWM` from the text of a status file,
/// where each is a line such as `VmRSS:` and `7824 kB`, a tab and spaces
/// between them.
fn parse(status: &str) -> Option<Memory> {
    let field = |name: &str| {
        let value = status
            .lines()
            .find_map(|line| line.strip_prefix(name)?.strip_prefix(':'))?;
        let kib: u64 = value.trim().strip_suffix("kB")?.trim().parse().ok()?;
        kib.checked_mul(1024)
    };
    Some(Memory {
        resident: field("VmRSS")?,
        anonymous: field("RssAnon")?,
        peak: field("VmHWM")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lines of a real status file, from Linux 7.2.
    const STATUS_TEXT: &str = "Name:\tsoak\n\
        VmPeak:\t  302244 kB\n\
        VmHWM:\t   15360 kB\n\
        VmRSS:\t   14540 kB\n\
        RssAnon:\t    6232 kB\n\
        RssFile:\t    8308 kB\n\
        Threads:\t5\n";

    #[test]
    fn the_three_fields_are_read_in_bytes() {
        assert_eq!(
            parse(STATUS_TEXT),
            Some(Memory {
                resident: 14_540 * 1024,
                anonymous: 6_232 * 1024,
                peak: 15_360 * 1024,
            })
        );
    }

    #[test]
    fn a_missing_or_unreadable_field_is_no_reading() {
        let without = |name: &str| {
            STATUS_TEXT
                .lines()
                .filter(|line| !line.starts_with(name))
                .collect::<Vec<_>>()
                .join("\n")
        };
        for name in ["VmRSS", "RssAnon", "VmHWM"] {
            assert_eq!(parse(&without(name)), None, "without {name}");
        }
        assert_eq!(parse(""), None);
        for line in [
            "VmRSS:\t 14540 MB",
            "VmRSS:\t lots kB",
            "VmRSS:\t -1 kB",
            "VmRSS 14540 kB",
            "VmRSS:\t 18014398509481984 kB",
        ] {
            let text = STATUS_TEXT.replace("VmRSS:\t   14540 kB", line);
            assert_eq!(parse(&text), None, "{line}");
        }
    }

    #[test]
    fn a_field_is_found_by_its_whole_name() {
        // `VmRSSx` is not `VmRSS`, and the first line of a name counts.
        let text = format!("VmRSSx:\t 1 kB\n{STATUS_TEXT}VmRSS:\t 2 kB\n");
        assert_eq!(
            parse(&text).map(|memory| memory.resident),
            Some(14_540 * 1024)
        );
    }

    #[test]
    fn this_process_is_read_where_there_is_proc() {
        let readable = fs::metadata(STATUS).is_ok();
        let memory = read();
        assert_eq!(memory.is_some(), readable);
        if let Some(memory) = memory {
            assert!(memory.anonymous > 0 && memory.anonymous <= memory.resident);
            assert!(memory.resident <= memory.peak);
        }
    }
}
