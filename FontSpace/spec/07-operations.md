# 7. Operations, Commands, Queries, and Transactions

## 7.1 Typed operations first

Every operation has a typed Rust request and result. A generic command dispatcher may wrap them, but the enum dispatcher is never the only API.

```rust
pub fn shift_glyphs(doc: &mut FontSpace, req: &ShiftGlyphs, ids: &mut dyn IdGen)
    -> Result<ChangeSet, FontSpaceError>;

pub fn extract_fragment(doc: &FontSpace, req: &ExtractFragment)
    -> Result<FontSpaceFragment, FontSpaceError>;

pub fn render_text_grid(doc: &FontSpace, req: &TextGridRequest)
    -> Result<String, FontSpaceError>;
```

Any operation that mints IDs takes `&mut dyn IdGen` (chapter 3). Queries never mutate and take `&FontSpace`.

## 7.2 Commands and queries

```rust
pub enum FontCommand {
    SetPixels(SetPixels),
    ShiftGlyphs(ShiftGlyphs),
    ClearGlyphs(ClearGlyphs),
    InvertGlyphs(InvertGlyphs),
    CopyGlyphs(CopyGlyphs),
    PasteFragment(PasteFragment),
    AddPage(AddPage),
    RemovePages(RemovePages),
    ReorderPages(ReorderPages),
    AddGuide(AddGuide),
    MoveGuide(MoveGuide),
    CopyGuideToPages(CopyGuideToPages),
    AddCharacterEntry(AddCharacterEntry),
    RemoveCharacterEntry(RemoveCharacterEntry),   // cascade-deletes referencing glyphs
    ReorderCharacterEntries(ReorderCharacterEntries),
    RecodeCharacterEntry(RecodeCharacterEntry),   // may orphan glyphs; warns
    AddExportConfig(AddExportConfig),
    ReplaceExportComponent(ReplaceExportComponent),
}

pub enum FontQuery {
    ExtractFragment(ExtractFragment),
    RenderTextGrid(TextGridRequest),
    RenderImage(ImageRenderRequest),
    InspectGlyph(InspectGlyph),
    InspectPages(InspectPages),
    ValidateExport(ValidateExportRequest),
    RenderLogicalMemoryImage(RenderMemoryImageRequest),
}
```

## 7.3 Selectors

Selectors are reusable across operations and are resolved-and-validated before any mutation begins.

```rust
pub enum PageSelector {
    All, Id(PageId), Ids(Vec<PageId>),
    Index(usize), RangeInclusive { start: usize, end: usize },
    Name(String), Names(Vec<String>),
}

pub enum GlyphSelector {
    All,
    Code(u32), Codes(Vec<u32>),
    CodeRangeInclusive { start: u32, end: u32 },
    Ordinal(usize), Ordinals(Vec<usize>),
    OrdinalRangeInclusive { start: usize, end: usize },
}
```

Glyphs are selected primarily by **`code`** (unique, unambiguous). `Ordinal` selectors resolve through the referenced character set's order. Name-based `PageSelector`s may be ambiguous (names are not unique); resolution rejects an ambiguous name with a precise error (chapter 14) rather than guessing. A `GlyphSelector` that names codes absent from the character set is an error unless the operation explicitly allows blank targets.

## 7.4 Pixel editing

```rust
pub struct GlyphRef { pub glyph_set_id: GlyphSetId, pub page_id: PageId, pub code: u32 }
pub struct PixelEdit { pub x: u16, pub y: u16, pub value: bool }
pub struct SetPixels { pub target: GlyphRef, pub edits: Vec<PixelEdit> }
```

`SetPixels` rejects out-of-bounds coordinates and may deduplicate repeated edits. If `target.code` has no glyph yet, the operation materializes a blank glyph, applies the edits, and records a `GlyphChanged` with `before = blank`.

## 7.5 Batch shift

```rust
pub struct ShiftGlyphs {
    pub glyph_set_id: GlyphSetId,
    pub pages: PageSelector,
    pub glyphs: GlyphSelector,
    pub dx: i16, pub dy: i16,
    pub overflow: OverflowPolicy,
}
pub enum OverflowPolicy { Discard, Wrap }
```

The initial UI may expose only `Discard`; the core supports both.

**Batch glyph transforms act on materialized glyphs only.** `ShiftGlyphs`, `ClearGlyphs`, and `InvertGlyphs` transform each **stored** glyph within the selection; a selected code with no stored glyph renders blank and is left untouched. In particular `InvertGlyphs` does **not** fill absent codes with all-on glyphs (that would materialize a glyph for every undrawn slot — e.g. an all-on glyph for all 128 ASCII codes). Only glyphs whose bitmap actually changes are recorded in the `ChangeSet`; a transform that leaves a glyph unchanged (a zero shift, clearing an already-blank glyph) contributes nothing.

## 7.6 Atomicity

Every batch operation is atomic:

1. resolve selectors;
2. validate all targets;
3. compute all outputs;
4. apply all changes;
5. return one `ChangeSet`.

If any target is invalid, the document is left unchanged. Cross-document operations use workspace transactions and are equally atomic.

## 7.7 Change sets and undo

```rust
pub struct ChangeSet {
    pub object_changes: Vec<ObjectChange>,
    pub warnings: Vec<FontSpaceWarning>,
}

pub enum ObjectChange {
    GlyphChanged(GlyphChange),
    GlyphRemoved(GlyphRemoved),       // e.g. entry-removal cascade
    PageInserted(PageInserted),
    PageRemoved(PageRemoved),
    GuideChanged(GuideChange),
    CharacterSetChanged(CharacterSetChange),
    ExportConfigChanged(ExportConfigChange),
}

pub struct GlyphChange {
    pub glyph_set_id: GlyphSetId,
    pub page_id: PageId,
    pub code: u32,
    pub before: Bitmap,   // blank if the glyph did not exist
    pub after: Bitmap,
}
```

Every change supports exact inversion; a `ChangeSet` is the undo/redo unit. **One pointer drag in the glyph editor is one `SetPixels` command and one undo entry** (chapter 12). The undo/redo **stack is workspace-level** (chapter 11), not per-document, because cross-document transactions must unwind both sides together; per-document dirty flags are updated from the documents a transaction touched.

## 7.8 The character-set cascade in change sets

`RemoveCharacterEntry` produces a `CharacterSetChanged` **plus** a `GlyphRemoved` for every cascade-deleted glyph across every referencing glyph set/page, all in one atomic `ChangeSet`, with a `warning` listing them (chapter 4 §4.4). Undo restores the entry and every removed glyph. `RecodeCharacterEntry` produces a `CharacterSetChanged` and warnings for any glyphs it orphaned, but moves no glyph data.

## 7.9 Workspace transactions

```rust
pub struct WorkspaceTransaction { pub document_changes: Vec<DocumentChangeSet> }
```

A move from document A to document B is one transaction whose undo/redo applies to both sides together (chapter 11).
