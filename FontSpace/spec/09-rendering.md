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
