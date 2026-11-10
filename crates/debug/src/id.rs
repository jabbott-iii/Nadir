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

//! Widget identity: a hash of the parent's ID and the widget's label or
//! explicit ID.

use std::hash::{Hash, Hasher};

use nadir_core::hash::FxLikeHasher;

/// A widget's, a window's or a scope's identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Id(u64);

impl Id {
    /// The parent of every window.
    pub(crate) const ROOT: Self = Self(0x6e61_6469_725f_6462);

    /// The ID of `key` under this one.
    pub(crate) fn with(self, key: impl Hash) -> Self {
        let mut hasher = FxLikeHasher::default();
        self.0.hash(&mut hasher);
        key.hash(&mut hasher);
        Self(hasher.finish())
    }
}

/// The parts of a window's, or a widget's, text: what is shown, before a
/// `##`, and its explicit ID, after it; with no `##`, both are the whole
/// text. `"Pause (3)##pause"` shows `Pause (3)` and keeps its identity
/// while the count changes.
pub(crate) fn split(text: &str) -> (&str, &str) {
    match text.find("##") {
        Some(at) => (&text[..at], &text[at + 2..]),
        None => (text, text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_explicit_id_is_kept_while_the_text_shown_changes() {
        assert_eq!(split("Pause (3)##pause"), ("Pause (3)", "pause"));
        assert_eq!(split("Reset"), ("Reset", "Reset"));
        assert_eq!(split("##hidden"), ("", "hidden"));
        let window = Id::ROOT.with("Stats");
        assert_eq!(
            window.with(split("Pause (3)##pause").1),
            window.with("pause")
        );
        assert_ne!(window.with("pause"), Id::ROOT.with("pause"), "by parent");
        assert_ne!(window.with("a"), window.with("b"));
    }
}
