//! Character sets: ordered, reusable lists of character slots (spec/04).
//!
//! The load-bearing decision (spec/04 §4.1): an entry's identity is its
//! **`code: u32`**, not its position (ordinal) and not a UUID. Reordering entries
//! changes ordinals but not codes, so glyph references never break; adding an entry
//! is a no-op for existing glyphs. `code` is also the ROM address dimension
//! (spec/10). `label` carries no identity and need not be unique.

use crate::{CharacterSetId, IdGen};

/// One character slot. Identified by `code`, which is required and unique within
/// its set (enforced by document validation, spec/14).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterEntry {
    /// Identity and ROM address dimension. The Unicode scalar for Unicode
    /// characters (`A` = `0x41`), a designer-chosen value (PUA `0xE000`+
    /// recommended) otherwise.
    pub code: u32,
    /// Human-readable name (e.g. `"NUL"`, `"LATIN CAPITAL A"`). Not an identity.
    pub label: String,
}

/// An ordered list of [`CharacterEntry`]s, reusable across glyph sets of different
/// geometry. `entries` order is canonical for both display and a page's on-disk
/// glyph order (spec/04 §4.2, spec/06).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterSet {
    pub id: CharacterSetId,
    pub name: String,
    pub description: String,
    pub entries: Vec<CharacterEntry>,
}

impl CharacterSet {
    /// A new, empty character set with a freshly-minted id.
    pub fn new(
        ids: &mut dyn IdGen,
        name: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            id: CharacterSetId::new(ids),
            name: name.into(),
            description: description.into(),
            entries: Vec::new(),
        }
    }

    /// The entry for `code`, if present.
    pub fn entry(&self, code: u32) -> Option<&CharacterEntry> {
        self.entries.iter().find(|entry| entry.code == code)
    }

    /// Whether `code` has an entry in this set (the referential-integrity check).
    pub fn contains_code(&self, code: u32) -> bool {
        self.entries.iter().any(|entry| entry.code == code)
    }

    /// The ordinal (position) of `code`, if present. Ordinal is implicit in order,
    /// never stored, and never what the ROM addresses (spec/04 §4.1).
    pub fn ordinal_of(&self, code: u32) -> Option<usize> {
        self.entries.iter().position(|entry| entry.code == code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SequentialIdGen;

    fn set_with_codes(codes: &[u32]) -> CharacterSet {
        let mut ids = SequentialIdGen::new();
        let mut cs = CharacterSet::new(&mut ids, "test", "");
        cs.entries = codes
            .iter()
            .map(|&code| CharacterEntry {
                code,
                label: format!("code {code:#04x}"),
            })
            .collect();
        cs
    }

    #[test]
    fn lookup_by_code_is_independent_of_ordinal() {
        // A printable-only set: ordinal 0 is code 0x20, not code 0 (spec/04 §4.1).
        let cs = set_with_codes(&[0x20, 0x21, 0x41]);
        assert_eq!(cs.ordinal_of(0x20), Some(0));
        assert_eq!(cs.ordinal_of(0x41), Some(2));
        assert!(cs.contains_code(0x21));
        assert!(!cs.contains_code(0x00));
        assert_eq!(cs.entry(0x41).map(|e| e.code), Some(0x41));
        assert_eq!(cs.entry(0x99), None);
        assert_eq!(cs.ordinal_of(0x99), None);
    }
}
