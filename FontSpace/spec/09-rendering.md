# 9. Rendering and Visualization

Rendering is a pure projection of selected FontSpace content. `fontspace-render` returns strings and image buffers using core value types; it never touches GUI textures. The GUI uploads buffers as textures, the CLI writes PNGs, MCP returns file references or encoded artifacts.

Absent glyphs (sparse pages, chapter 5) render blank. Dangling glyphs (chapter 4) render blank and are flagged in inspection, never in the projection.

## 9.1 Core color value type

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgba { pub r: u8, pub g: u8, pub b: u8, pub a: u8 }
```

This is the boundary type; `egui::Color32` never crosses into core crates.

## 9.2 Text-grid rendering

```rust
pub struct TextGridRequest {
    pub glyph_set_id: GlyphSetId,
    pub pages: PageSelector,
    pub glyphs: GlyphSelector,
    pub on: String,
    pub off: String,
    pub glyph_separator: String,
    pub row_separator: String,
    pub page_separator: String,
    pub layout: RenderLayout,
    pub scale_x: usize,
    pub scale_y: usize,
}

pub enum RenderLayout {
    GlyphsHorizontal, GlyphsVertical,
    Grid { columns: usize },
    PagesHorizontal, PagesVertical,
}
```

`on`/`off` are arbitrary strings, so a glyph can be rendered as `#`/`.`, block characters, or emoji. `scale_x`/`scale_y` repeat the on/off tokens per pixel.

**Composition.** Each glyph renders to a block of visual rows. A page's glyph blocks are arranged per `layout`: `GlyphsHorizontal` (and the per-page layout used by `PagesHorizontal`/`PagesVertical`) places them side by side, joining each row with `glyph_separator`; `GlyphsVertical` stacks them separated by a single `glyph_separator` row; `Grid { columns }` wraps into rows of `columns`, joining with `glyph_separator` both across and between rows. Multiple pages stack vertically separated by a `page_separator` row, except `PagesHorizontal`, which places pages side by side joined by `page_separator`. `PagesVertical` lays each page's glyphs out horizontally (so with a single selected page it renders identically to `GlyphsHorizontal`; the variants diverge only once there is more than one page). All resulting rows are finally joined by `row_separator` (typically `"\n"`). Absent codes render blank, so a selected code with no glyph still occupies its cell.

### 9.2.1 Text-string rendering

A second text projection renders an **ordered run** of glyphs addressed by character `code`, rather than a set-selection. This is what the CLI's `--text` / `--text-nl` use (spec/13.1) and the model for text preview (§9.4).

```rust
pub struct TextStringRequest {
    pub glyph_set_id: GlyphSetId,
    pub pages: PageSelector,
    pub rows: Vec<Vec<u32>>,        // lines of character codes, in order
    pub on: String,
    pub off: String,
    pub glyph_separator: String,
    pub row_separator: String,
    pub page_separator: String,
    pub scale_x: usize,
    pub scale_y: usize,
}
```

It differs from §9.2 in two ways. First, `rows` is an **ordered sequence with repeats** — the same `code` may appear many times and renders each time — where a `GlyphSelector` resolves to a deduplicated set. Each inner vector is one output line; lines stack directly (a source newline begins a new line), and pages stack vertically joined by `page_separator`. An empty or all-ignored line contributes no rows (there is no blank-gap line). Second, membership is **entry-gated**: a `code` renders iff the glyph set's character set has an entry for it; a code with no entry is **ignored** (it is not a character of this font). A rendered code whose glyph is absent on the page draws blank (the §9.2 absent-as-blank rule), so a defined-but-empty character such as a space still occupies its cell. `on`/`off`/`scale_*`/`glyph_separator` behave as in §9.2.

## 9.3 Image rendering

```rust
pub struct ImageRenderRequest {
    pub glyph_set_id: GlyphSetId,
    pub pages: PageSelector,
    pub glyphs: GlyphSelector,
    pub layout: RenderLayout,
    pub pixel_scale: u32,
    pub cell_gap: u32,
    pub glyph_gap: u32,
    pub foreground: Rgba,
    pub background: Rgba,
    pub show_grid: bool,
    pub show_labels: bool,
}
```

Returns an owned image buffer (dimensions + `Vec<u8>` RGBA), independent of GUI types.

## 9.4 Text-preview rendering

Text preview maps an input string through a chosen character set and page: each input character's Unicode scalar is matched against entry `code`s; the matched code's glyph (or blank) is emitted.

```rust
pub enum MissingGlyphPolicy { Blank, ReplacementCode(u32), Error }
```

Preview options: page selection, foreground/background inversion, integer zoom, line spacing, character spacing, wrap width, optional cell-grid overlay. A multi-page comparison mode renders the same text once per selected page.
