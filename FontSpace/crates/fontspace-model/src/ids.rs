//! Typed IDs and the injected UUID source.
//!
//! Durable, independently-selectable objects carry newtype-wrapped UUIDs that are
//! **not** interchangeable — the type system distinguishes a `PageId` from a
//! `GlyphSetId`. Character entries are the deliberate exception: they are keyed by
//! their `code: u32`, not a UUID (see spec/03 §3.2 and spec/04).
//!
//! IDs are never derived from vector indexes or names. New IDs come only from an
//! injected [`IdGen`]; no core code calls `Uuid::new_v4()` directly, which is what
//! makes created documents byte-for-byte reproducible under `SequentialIdGen`
//! (spec/03 §3.3, the determinism invariant in spec/17).

use uuid::Uuid;

/// The injected UUID source threaded through every ID-minting operation.
///
/// Production wires [`RandomIdGen`]; tests wire [`SequentialIdGen`] so documents
/// are reproducible and comparable against golden JSON.
pub trait IdGen {
    fn next_uuid(&mut self) -> Uuid;
}

/// Production generator: real random v4 UUIDs.
#[derive(Debug, Default, Clone, Copy)]
pub struct RandomIdGen;

impl IdGen for RandomIdGen {
    fn next_uuid(&mut self) -> Uuid {
        Uuid::new_v4()
    }
}

/// Test generator yielding `00000000-0000-0000-0000-000000000001`, `…0002`, ….
///
/// Deterministic and reproducible — the only randomness/uniqueness source allowed
/// in tests (spec/15). Never wire this in production.
#[derive(Debug, Clone)]
pub struct SequentialIdGen {
    next: u128,
}

impl SequentialIdGen {
    /// Starts the sequence at `…0001` (never the nil UUID).
    pub fn new() -> Self {
        Self { next: 1 }
    }
}

impl Default for SequentialIdGen {
    fn default() -> Self {
        Self::new()
    }
}

impl IdGen for SequentialIdGen {
    fn next_uuid(&mut self) -> Uuid {
        let uuid = Uuid::from_u128(self.next);
        self.next += 1;
        uuid
    }
}

/// Declares a newtype-wrapped-UUID id type with a common surface: construction
/// from an [`IdGen`], and access to the underlying [`Uuid`] without exposing a
/// second way to mint one.
macro_rules! typed_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub struct $name(pub Uuid);

        impl $name {
            /// Mints a fresh id from the injected generator.
            pub fn new(ids: &mut dyn IdGen) -> Self {
                Self(ids.next_uuid())
            }

            /// The underlying UUID (for serialization and comparison only).
            pub fn as_uuid(&self) -> Uuid {
                self.0
            }
        }
    };
}

typed_id!(
    /// Identifies a [`FontSpace`](crate::FontSpace) document object.
    FontSpaceId
);
typed_id!(
    /// Identifies a `CharacterSet` within a document (spec/04).
    CharacterSetId
);
typed_id!(
    /// Identifies a `GlyphSet` within a document (spec/03 §3.5).
    GlyphSetId
);
typed_id!(
    /// Identifies a `GlyphPage` within a glyph set (spec/03 §3.6).
    PageId
);
typed_id!(
    /// Identifies a `Guide` on a page (spec/03 §3.8).
    GuideId
);
typed_id!(
    /// Identifies an `ExportConfig` within a document (spec/10).
    ExportConfigId
);
typed_id!(
    /// Identifies an export component within an export config (spec/10).
    ExportComponentId
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequential_id_gen_starts_at_one_and_increments() {
        let mut ids = SequentialIdGen::new();
        assert_eq!(
            ids.next_uuid(),
            Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap()
        );
        assert_eq!(
            ids.next_uuid(),
            Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap()
        );
        assert_eq!(
            ids.next_uuid(),
            Uuid::parse_str("00000000-0000-0000-0000-000000000003").unwrap()
        );
    }

    #[test]
    fn sequential_id_gen_is_reproducible() {
        let mut a = SequentialIdGen::new();
        let mut b = SequentialIdGen::new();
        for _ in 0..10 {
            assert_eq!(a.next_uuid(), b.next_uuid());
        }
    }

    #[test]
    fn typed_ids_of_same_uuid_are_equal_and_hash_together() {
        use std::collections::HashSet;
        let mut ids = SequentialIdGen::new();
        let a = PageId::new(&mut ids);
        let b = PageId(a.as_uuid());
        assert_eq!(a, b);
        let mut set = HashSet::new();
        set.insert(a);
        assert!(set.contains(&b));
    }

    #[test]
    fn distinct_typed_ids_get_distinct_uuids_from_one_gen() {
        let mut ids = SequentialIdGen::new();
        let g = GlyphSetId::new(&mut ids);
        let p = PageId::new(&mut ids);
        assert_ne!(g.as_uuid(), p.as_uuid());
    }

    #[test]
    fn random_id_gen_yields_distinct_nonnil_uuids() {
        let mut ids = RandomIdGen;
        let a = ids.next_uuid();
        let b = ids.next_uuid();
        assert_ne!(a, b);
        assert_ne!(a, Uuid::nil());
    }
}
