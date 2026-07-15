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

A fragment carries enough dependency metadata to resolve referenced character sets and source geometry. A `GlyphFragment` records the source `glyph_size` and, per glyph, its `code` and `label` (so paste can map by code or by slot into a destination whose character set differs):

```rust
pub struct GlyphFragment {
    pub source_glyph_size: GlyphSize,
    pub glyphs: Vec<FragmentGlyph>,   // invariant: every bitmap is source_glyph_size
}

pub struct FragmentGlyph {
    pub code: u32,
    pub label: String,
    pub bitmap: Bitmap,
}
```

Fragment types live in `fontspace-model`; the `extract`/`paste` operations that produce and consume them live in `fontspace-ops` (spec/02). The variants land incrementally — `Glyphs` first, then `Pages`, `Objects`, and `ExportComponents` — so at any milestone `FontSpaceFragment` may carry only the subset built so far.

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

The default `GlyphSizeConversion` is `RequireExact`: pasting between incompatible geometries fails with a clear error unless the user chooses a conversion. No silent resize, ever. `RequireExact` checks both the fragment's declared `source_glyph_size` and every glyph's actual bitmap size against the destination geometry, so a malformed fragment (§8.2, e.g. from an untrusted clipboard or file) cannot slip a wrong-size glyph past the geometry-agreement invariant (spec/17).

The conversions are **lossless placements** — they copy the source pixels into a blank destination-size glyph, clipping whatever falls outside; none resamples. `PlaceAt { x, y }` puts the source's top-left pixel at `(x, y)` (offsets may be negative, so the source is cropped from the top/left). `Center` places it with equal margins, cropping symmetrically when the source is larger; an odd size difference floors toward the top-left. `Crop` (an explicit top-left/region crop) and `ScaleNearest` (the one resampling conversion) arrive with a later slice.

Two further paste rules follow from the model: a paste **never creates a dangling glyph** — every resolved destination `code` must already have an entry in the destination character set (the same rule operations obey, spec/04 §4.4) — and a `SequentialFromCode` run that would exceed the 32-bit code space is rejected, not wrapped or saturated. Like every operation, a paste validates all targets before mutating and returns one invertible change set (spec/07).

The policy variants land incrementally alongside the fragment variants: the glyph paste supports `ByCode` and `SequentialFromCode` mappings and `RequireExact`, `PlaceAt`, and `Center` conversions. `BySlot`, `Crop`, and `ScaleNearest` arrive with later slices.

When pasting a glyph set into another document, its character-set dependency must be resolved explicitly: reused if the user selects an equivalent destination set, copied as a new object, explicitly rebound, or rejected. FontSpace never invents a mapping.

## 8.4 Common cross-file workflows (must be easy)

- copy one glyph from file A to file B;
- copy a code range (e.g. `A`–`Z`) from several pages in file A to file B;
- duplicate a page into another glyph set of compatible geometry;
- combine pages from two files into a third;
- copy an export config between files;
- copy only an address map or data map between configs;
- compare equivalent characters across files without copying (chapter 12 §comparison view).

## 8.5 Fragment JSON

The authoritative clipboard representation (§8.2) and the on-disk form the CLI reads and writes is canonical JSON, produced by `fontspace-json` (`save_fragment`/`load_fragment`). It follows the **same** conventions and determinism contract as document JSON (spec/06): explicit storage structs fix field order, `code`s are lowercase `0x` hex, bitmaps are `.`/`#` visual rows, indentation is two spaces, and there is exactly one trailing newline. The contract is `save_fragment(load_fragment(save_fragment(f))) == save_fragment(f)`, byte-for-byte.

A fragment is tagged by a `fragment_version` (migrated independently of the document `format_version`) and a `kind` that dispatches the variant. The glyph fragment (`kind: "glyphs"`) is:

```json
{
  "fragment_version": 1,
  "kind": "glyphs",
  "source_glyph_size": { "width": 5, "height": 3 },
  "glyphs": [
    { "code": "0x41", "label": "LATIN CAPITAL A", "pixels": [".###.", "#...#", "#####"] }
  ]
}
```

Each glyph carries exactly `source_glyph_size.height` `pixels` rows, each exactly `width` characters — here a 5×3 `A`.

On load, an unsupported `fragment_version` or an unknown `kind` is rejected; the `source_glyph_size` is validated (spec/03 §3.4); and every glyph's `pixels` must match that geometry, so the `GlyphFragment` per-glyph size invariant (§8.1) holds by construction. Loading never guesses — a malformed fragment is a clear error, not a coerced value. The `Pages`, `Objects`, and `ExportComponents` kinds arrive with their fragment variants.
