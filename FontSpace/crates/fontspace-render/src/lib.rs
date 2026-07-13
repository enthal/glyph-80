#![forbid(unsafe_code)]

//! Text-grid rendering (spec/09): a pure projection of selected glyphs to a string.
//! Returns text, never GUI textures (the inward-dependency rule, spec/02). Absent
//! glyphs render blank (spec/05 §5.6).
//!
//! ## Composition (spec/09 §9.2)
//! Each glyph renders to a block of visual rows: every pixel becomes its `on`/`off`
//! token repeated `scale_x` times, and each row repeated `scale_y` times. Blocks are
//! then arranged:
//! - `GlyphsHorizontal` / `PagesHorizontal` / `PagesVertical` place a page's glyphs
//!   side by side, joined on each row by `glyph_separator`;
//! - `GlyphsVertical` stacks a page's glyphs, separated by a `glyph_separator` row;
//! - `Grid { columns }` wraps glyphs into rows of `columns`, joined by
//!   `glyph_separator` both across and between rows.
//!
//! Multiple pages stack vertically separated by a `page_separator` row, except
//! `PagesHorizontal`, which places pages side by side joined by `page_separator`.
//! Finally all rows are joined by `row_separator` (typically `"\n"`).

use fontspace_model::{Bitmap, FontSpace, GlyphSetId};
use fontspace_ops::{
    FontSpaceError, GlyphSelector, PageSelector, resolve_glyph_codes_in, resolve_pages_in,
};

/// How glyphs and pages are arranged in a text-grid render (spec/09 §9.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderLayout {
    GlyphsHorizontal,
    GlyphsVertical,
    Grid { columns: usize },
    PagesHorizontal,
    PagesVertical,
}

/// A request to render selected glyphs as a text grid (spec/09 §9.2). `on`/`off` are
/// arbitrary strings, so a glyph can be drawn as `#`/`.`, block characters, or emoji.
#[derive(Debug, Clone, PartialEq, Eq)]
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

/// A rectangular-ish block of visual rows.
type Block = Vec<String>;

/// Render selected glyphs to a text grid (spec/09 §9.2). Absent codes render blank.
pub fn render_text_grid(doc: &FontSpace, req: &TextGridRequest) -> Result<String, FontSpaceError> {
    let page_ids = resolve_pages_in(doc, req.glyph_set_id, &req.pages)?;
    let codes = resolve_glyph_codes_in(doc, req.glyph_set_id, &req.glyphs)?;
    let glyph_set = doc
        .glyph_set(req.glyph_set_id)
        .ok_or(FontSpaceError::GlyphSetNotFound(req.glyph_set_id))?;
    let size = glyph_set.glyph_size;

    let mut page_blocks: Vec<Block> = Vec::new();
    for page_id in &page_ids {
        // Resolved above, so the page exists.
        let Some(page) = glyph_set.page_of_id(*page_id) else {
            continue;
        };
        let glyph_blocks: Vec<Block> = codes
            .iter()
            .map(|&code| {
                let bitmap = page
                    .glyph_of_code(code)
                    .map(|glyph| glyph.bitmap.clone())
                    .unwrap_or_else(|| Bitmap::new_blank(size));
                render_glyph(&bitmap, &req.on, &req.off, req.scale_x, req.scale_y)
            })
            .collect();
        page_blocks.push(arrange_glyphs(glyph_blocks, req));
    }

    let combined = match req.layout {
        RenderLayout::PagesHorizontal => hstack(&page_blocks, &req.page_separator),
        _ => vstack(&page_blocks, &req.page_separator),
    };
    Ok(combined.join(&req.row_separator))
}

/// Renders one glyph to a block: each pixel → its `on`/`off` token repeated `scale_x`
/// times, each row repeated `scale_y` times.
fn render_glyph(bitmap: &Bitmap, on: &str, off: &str, scale_x: usize, scale_y: usize) -> Block {
    let mut rows = Vec::with_capacity(bitmap.height() as usize * scale_y);
    for y in 0..bitmap.height() {
        let mut row = String::new();
        for x in 0..bitmap.width() {
            let token = if bitmap.get(x, y).unwrap_or(false) {
                on
            } else {
                off
            };
            for _ in 0..scale_x {
                row.push_str(token);
            }
        }
        for _ in 0..scale_y {
            rows.push(row.clone());
        }
    }
    rows
}

fn arrange_glyphs(glyph_blocks: Vec<Block>, req: &TextGridRequest) -> Block {
    match &req.layout {
        RenderLayout::GlyphsVertical => vstack(&glyph_blocks, &req.glyph_separator),
        RenderLayout::Grid { columns } => grid(&glyph_blocks, *columns, &req.glyph_separator),
        // Horizontal glyph strip (also the per-page layout for the Pages* modes).
        _ => hstack(&glyph_blocks, &req.glyph_separator),
    }
}

/// Places blocks side by side, joining each row by `separator`. Blocks shorter than
/// the tallest are padded with empty rows so no row is dropped.
fn hstack(blocks: &[Block], separator: &str) -> Block {
    if blocks.is_empty() {
        return Vec::new();
    }
    let height = blocks.iter().map(|block| block.len()).max().unwrap_or(0);
    (0..height)
        .map(|r| {
            blocks
                .iter()
                .map(|block| block.get(r).map(String::as_str).unwrap_or(""))
                .collect::<Vec<_>>()
                .join(separator)
        })
        .collect()
}

/// Stacks blocks, inserting a single `separator` row between them.
fn vstack(blocks: &[Block], separator: &str) -> Block {
    let mut out = Vec::new();
    for (i, block) in blocks.iter().enumerate() {
        if i > 0 {
            out.push(separator.to_string());
        }
        out.extend(block.iter().cloned());
    }
    out
}

/// Arranges blocks into a grid of `columns` columns: each row of glyphs is
/// horizontally stacked, and the rows are vertically stacked, all joined by
/// `separator`. `columns == 0` is treated as a single row.
fn grid(blocks: &[Block], columns: usize, separator: &str) -> Block {
    let columns = columns.max(1);
    let rows: Vec<Block> = blocks
        .chunks(columns)
        .map(|chunk| hstack(chunk, separator))
        .collect();
    vstack(&rows, separator)
}

#[cfg(test)]
mod tests;
