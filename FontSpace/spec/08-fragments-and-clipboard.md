# 8. Fragments, Clipboard, and Cross-File Composition

## 8.1 General rule

Copying is based on serializable **domain fragments**, not GUI widget state. A fragment can represent one glyph, a code range, one or more pages, a glyph set, a character set, an export config, a single export component, or multiple heterogeneous objects.

```rust
pub enum FontSpaceFragment {
    Objects(Vec<FontSpaceObject>),
    Glyphs(GlyphFragment),
    Pages(PageFragment),
    ExportComponents(Vec<ExportComponentFragment>),
}

pub enum FontSpaceObject {
    CharacterSet(CharacterSet),
    GlyphSet(GlyphSet),
    ExportConfig(ExportConfig),
}
```

A fragment carries enough dependency metadata to resolve referenced character sets and source geometry. A `GlyphFragment` records the source `glyph_size` and, per glyph, its `code` and `label` (so paste can map by code or by slot into a destination whose character set differs).

## 8.2 Clipboard representations

Where the platform supports it, copying places multiple formats on the system clipboard, richest first:

```text
application/x-fontspace-fragment+json   (authoritative; preferred on paste)
text/plain                              (visual grid / readable JSON — lossy fallback)
image/png                               (rendered preview)
```

A receiving FontSpace instance prefers the custom fragment type. Cross-instance rich paste depends on it; a `text/plain` paste is best-effort and loses geometry and dependency metadata. Clipboard integration lives only in `fontspace-egui`; the core produces the fragment and the rendered bytes.

## 8.3 Paste policies — never guess silently

```rust
pub enum GlyphMapping {
    ByCode,                      // destination code == source code
    BySlot,                      // destination ordinal == source ordinal
    SequentialFromCode(u32),
}

pub enum PageMapping {
    IntoCurrentPage, ByName, SequentialFromPage(usize), CreateNewPages,
}

pub enum GlyphSizeConversion {
    RequireExact,                // default
    PlaceAt { x: i16, y: i16 },
    Center, Crop, ScaleNearest,
}
```

The default `GlyphSizeConversion` is `RequireExact`: pasting between incompatible geometries fails with a clear error unless the user chooses a conversion. No silent resize, ever.

When pasting a glyph set into another document, its character-set dependency must be resolved explicitly: reused if the user selects an equivalent destination set, copied as a new object, explicitly rebound, or rejected. FontSpace never invents a mapping.

## 8.4 Common cross-file workflows (must be easy)

- copy one glyph from file A to file B;
- copy a code range (e.g. `A`–`Z`) from several pages in file A to file B;
- duplicate a page into another glyph set of compatible geometry;
- combine pages from two files into a third;
- copy an export config between files;
- copy only an address map or data map between configs;
- compare equivalent characters across files without copying (chapter 12 §comparison view).
