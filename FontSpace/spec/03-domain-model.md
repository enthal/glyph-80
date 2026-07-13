# 3. Domain Model

## 3.1 FontSpace document

A FontSpace document is one JSON file containing named, stable objects.

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontSpace {
    pub format_version: u32,
    pub id: FontSpaceId,
    pub metadata: FontSpaceMetadata,
    pub character_sets: Vec<CharacterSet>,
    pub glyph_sets: Vec<GlyphSet>,
    pub export_configs: Vec<ExportConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontSpaceMetadata {
    pub name: String,
    pub description: String,
}
```

There is **no** global glyph size and **no** global character set at the document level. A document may contain only character sets, only glyph sets and their referenced character sets, only export configs, or any combination.

## 3.2 Typed IDs

Objects that can be selected, reordered, referenced, copied, or edited independently have stable IDs. IDs are newtype-wrapped UUIDs and are not interchangeable.

```rust
pub struct FontSpaceId(pub Uuid);
pub struct CharacterSetId(pub Uuid);   // referenced by GlyphSet (§3.5)
pub struct GlyphSetId(pub Uuid);
pub struct PageId(pub Uuid);
pub struct GuideId(pub Uuid);
pub struct ExportConfigId(pub Uuid);
pub struct ExportComponentId(pub Uuid);
```

Each derives `Debug, Clone, Copy, PartialEq, Eq, Hash` (and `Serialize`/`Deserialize` via the storage layer). IDs are never derived from vector indexes or names. Names are user-facing labels and need not be unique unless a specific operation requires disambiguation.

**Character entries are the deliberate exception:** they carry no UUID and are keyed by their `code: u32` (chapter 4). `code` is a natural, unique, human-meaningful, hardware-meaningful key; a UUID would add diff noise for no gain.

`DocumentId` (chapter 11) is a **runtime-only** identity for an open document. It is never persisted inside domain references.

## 3.3 Id injection and determinism

No core crate calls `Uuid::new_v4()`, a clock, or unseeded RNG. Object creation takes an injected generator:

```rust
pub trait IdGen {
    fn next_uuid(&mut self) -> Uuid;
}

/// Production: real random v4 UUIDs.
pub struct RandomIdGen;

/// Tests: 00000000-0000-0000-0000-000000000001, ...0002, ...
pub struct SequentialIdGen { next: u128 }
```

Every operation that mints an ID takes `&mut dyn IdGen` (or a generic `G: IdGen`). Tests wire `SequentialIdGen` so created documents are byte-for-byte reproducible and can be compared against golden JSON. Any future need for randomness or time (e.g. recovery-snapshot naming) is injected the same way — a `Clock`/`Rng` trait, never an ambient call. See [15-testing.md](15-testing.md).

## 3.4 Glyph geometry

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlyphSize {
    pub width: u16,
    pub height: u16,
}
```

Both dimensions must be nonzero and bounded by the limits in [16-performance-safety-limits.md](16-performance-safety-limits.md) to prevent pathological allocations.

## 3.5 Glyph sets

A glyph set fixes one geometry, references one character set, and holds zero or more pages.

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlyphSet {
    pub id: GlyphSetId,
    pub name: String,
    pub description: String,
    pub glyph_size: GlyphSize,
    pub character_set_id: CharacterSetId,
    pub pages: Vec<GlyphPage>,
}
```

`CharacterSetId` is a UUID identifying a `CharacterSet` object within the same document (chapter 4). Multiple glyph sets of different geometry may reference the same character set.

Example objects within one file:

```text
Terminal 8×16      Compact 8×8      Icons 16×16
  Regular            Main             Hardware Symbols
  Bold
  Alternate
```

## 3.6 Pages

A page belongs to a glyph set and holds a **sparse** set of glyphs — one glyph for each `code` it defines, and nothing for the rest (absent = blank). Glyphs are keyed by the entry `code` they render.

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlyphPage {
    pub id: PageId,
    pub name: String,
    pub description: String,
    pub guides: Vec<Guide>,
    pub glyphs: Vec<Glyph>,   // sparse; each Glyph carries its `code`; see ordering below
}
```

The in-memory `glyphs` collection is indexed by `code` for lookup (`glyph_by_code`), but its **canonical order is charset-entry order** (chapter 4 §Ordering) so that storage and display are stable and diffable. Page order within a glyph set is significant and may later participate in export ordering.

There is no `page.glyphs.len() == entries.len()` invariant. The invariants that replace it (chapter 4 §Invariants) are referential integrity (every glyph's `code` exists in the referenced character set) and uniqueness (at most one glyph per `code` per page). A glyph whose `code` is absent from the character set is **dangling** — tolerated with a warning on load, and produced only transiently (an entry deletion cascade removes such glyphs; see chapter 4).

## 3.7 Glyphs

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Glyph {
    pub code: u32,      // the character-set entry this glyph renders
    pub bitmap: Bitmap, // geometry must match the owning glyph set's glyph_size
}
```

Pixel meaning is fixed: `false` = off, `true` = on. Display inversion is a view concern and is never stored. The `Bitmap` type, its packing, and the blank/sparse semantics are specified in [05-glyphs-and-bitmaps.md](05-glyphs-and-bitmaps.md).

## 3.8 Guides

Guides are named integer coordinates associated with a page.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuideAxis { Horizontal, Vertical }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Guide {
    pub id: GuideId,
    pub name: String,
    pub axis: GuideAxis,
    pub position: i32,   // a grid-line coordinate (between pixels), signed; may lie outside glyph bounds
    pub visible: bool,
    pub locked: bool,
}
```

Typical guides: baseline, cap height, x-height, left/right edge, nominal center, hardware-specific boundaries. Copying a guide to another page mints a **fresh** `GuideId` (chapter 7); IDs never collide across pages.
