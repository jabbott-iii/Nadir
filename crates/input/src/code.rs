//! Keys and mouse buttons as small numbers, and sets of them.

use nadir_core::{Key, MouseButton};

/// A key or a mouse button as a number: a key's HID usage, below `0x100`,
/// or `0x100` plus a mouse button's position in [`MouseButton::ALL`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Code(u16);

/// Where the mouse buttons' codes start.
const MOUSE: u16 = 0x100;

impl Code {
    /// The code of a key.
    pub(crate) const fn key(key: Key) -> Self {
        Self(key.usage() as u16)
    }

    /// The code of a mouse button.
    pub(crate) const fn mouse(button: MouseButton) -> Self {
        Self(MOUSE + button as u16)
    }

    const fn word_and_bit(self) -> (usize, u64) {
        ((self.0 / 64) as usize, 1 << (self.0 % 64))
    }
}

/// Enough 64-bit words for every code: the last mouse button's is
/// `0x104`.
const WORDS: usize = 5;

/// A set of codes, with no allocation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct CodeSet([u64; WORDS]);

impl CodeSet {
    pub(crate) fn contains(&self, code: Code) -> bool {
        let (word, bit) = code.word_and_bit();
        self.0[word] & bit != 0
    }

    pub(crate) fn insert(&mut self, code: Code) {
        let (word, bit) = code.word_and_bit();
        self.0[word] |= bit;
    }

    pub(crate) fn remove(&mut self, code: Code) {
        let (word, bit) = code.word_and_bit();
        self.0[word] &= !bit;
    }

    /// Every code in the set, in order.
    pub(crate) fn iter(&self) -> impl Iterator<Item = Code> + '_ {
        self.0.iter().enumerate().flat_map(|(word, &bits)| {
            (0..64u16)
                .filter(move |bit| bits & (1 << bit) != 0)
                .map(move |bit| Code(word as u16 * 64 + bit))
        })
    }
}

/// The codes a binding reads: one, or four for a composite.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Inputs {
    codes: [Code; 4],
    len: usize,
}

impl Inputs {
    pub(crate) const fn one(code: Code) -> Self {
        Self {
            codes: [code; 4],
            len: 1,
        }
    }

    pub(crate) const fn four(codes: [Code; 4]) -> Self {
        Self { codes, len: 4 }
    }

    pub(crate) fn as_slice(&self) -> &[Code] {
        &self.codes[..self.len]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_and_button_has_its_own_code_within_the_set() {
        let codes: Vec<Code> = Key::ALL
            .iter()
            .map(|&key| Code::key(key))
            .chain(MouseButton::ALL.iter().map(|&button| Code::mouse(button)))
            .collect();
        let mut set = CodeSet::default();
        for &code in &codes {
            assert!(!set.contains(code), "{code:?} twice");
            set.insert(code);
        }
        assert_eq!(set.iter().collect::<Vec<_>>(), codes);
        assert!(Code::mouse(MouseButton::Forward).0 < (WORDS * 64) as u16);
    }

    #[test]
    fn removing_leaves_the_others() {
        let mut set = CodeSet::default();
        set.insert(Code::key(Key::W));
        set.insert(Code::mouse(MouseButton::Left));
        set.remove(Code::key(Key::W));
        assert!(!set.contains(Code::key(Key::W)));
        assert_eq!(
            set.iter().collect::<Vec<_>>(),
            [Code::mouse(MouseButton::Left)]
        );
    }
}
