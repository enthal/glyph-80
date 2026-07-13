//! Pixel and glyph commands (spec/07 §7.4–7.5): `SetPixels`, `ShiftGlyphs`,
//! `ClearGlyphs`, `InvertGlyphs`. Each resolves and validates targets, computes all
//! outputs, then applies atomically, returning one invertible [`ChangeSet`].

use fontspace_model::{Bitmap, FontSpace, GlyphSetId, OverflowPolicy, PageId, inverted, shifted};

use crate::apply_change_set;
use crate::change_set::{ChangeSet, GlyphChange, ObjectChange};
use crate::error::FontSpaceError;
use crate::selector::{GlyphSelector, PageSelector, resolve_glyph_codes, resolve_pages};

/// A single glyph, addressed for editing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlyphRef {
    pub glyph_set_id: GlyphSetId,
    pub page_id: PageId,
    pub code: u32,
}

/// One pixel assignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelEdit {
    pub x: u16,
    pub y: u16,
    pub value: bool,
}

/// Set individual pixels on one glyph. Materializes a blank glyph if the target code
/// has none yet (recording `before = blank`), and rejects out-of-bounds coordinates
/// (spec/07 §7.4). A whole editor drag is one `SetPixels` — one undo entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetPixels {
    pub target: GlyphRef,
    pub edits: Vec<PixelEdit>,
}

/// Translate selected glyphs by `(dx, dy)` under `overflow` (spec/07 §7.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShiftGlyphs {
    pub glyph_set_id: GlyphSetId,
    pub pages: PageSelector,
    pub glyphs: GlyphSelector,
    pub dx: i16,
    pub dy: i16,
    pub overflow: OverflowPolicy,
}

/// Blank selected glyphs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClearGlyphs {
    pub glyph_set_id: GlyphSetId,
    pub pages: PageSelector,
    pub glyphs: GlyphSelector,
}

/// Invert (toggle every pixel of) selected glyphs — a data operation, distinct from
/// display inversion (spec/05 §5.7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvertGlyphs {
    pub glyph_set_id: GlyphSetId,
    pub pages: PageSelector,
    pub glyphs: GlyphSelector,
}

/// Applies `SetPixels`, returning an invertible change set (empty if a no-op).
pub fn set_pixels(doc: &mut FontSpace, req: &SetPixels) -> Result<ChangeSet, FontSpaceError> {
    let glyph_set_id = req.target.glyph_set_id;
    let change = {
        let glyph_set = doc
            .glyph_set(glyph_set_id)
            .ok_or(FontSpaceError::GlyphSetNotFound(glyph_set_id))?;
        let character_set = doc.character_set(glyph_set.character_set_id).ok_or(
            FontSpaceError::CharacterSetNotFound {
                glyph_set: glyph_set_id,
                character_set: glyph_set.character_set_id,
            },
        )?;
        let Some(page) = glyph_set.page_of_id(req.target.page_id) else {
            return Err(FontSpaceError::PageIdNotFound {
                glyph_set: glyph_set_id,
                page: req.target.page_id,
            });
        };
        // Editing via an operation never creates a dangling glyph: the target code
        // must have an entry (spec/04 §4.4; dangling is tolerated only on load).
        if !character_set.contains_code(req.target.code) {
            return Err(FontSpaceError::CodeNotInCharacterSet {
                character_set: glyph_set.character_set_id,
                code: req.target.code,
            });
        }
        let size = glyph_set.glyph_size;
        let before = page
            .glyph_of_code(req.target.code)
            .map(|glyph| glyph.bitmap.clone())
            .unwrap_or_else(|| Bitmap::new_blank(size));

        // Validate all edits against the geometry before committing anything.
        let mut after = before.clone();
        for edit in &req.edits {
            after.set(edit.x, edit.y, edit.value).map_err(|_| {
                FontSpaceError::PixelOutOfBounds {
                    context: format!(
                        "glyph set {glyph_set_id:?} / page {:?} / code {:#06x}",
                        req.target.page_id, req.target.code
                    ),
                    x: edit.x,
                    y: edit.y,
                    width: size.width,
                    height: size.height,
                }
            })?;
        }
        if after == before {
            return Ok(ChangeSet::default());
        }
        GlyphChange {
            glyph_set_id,
            page_id: req.target.page_id,
            code: req.target.code,
            before,
            after,
        }
    };

    let change_set = ChangeSet {
        object_changes: vec![ObjectChange::GlyphChanged(change)],
        warnings: Vec::new(),
    };
    apply_change_set(doc, &change_set)?;
    Ok(change_set)
}

/// Applies `ShiftGlyphs`.
pub fn shift_glyphs(doc: &mut FontSpace, req: &ShiftGlyphs) -> Result<ChangeSet, FontSpaceError> {
    batch_transform(doc, req.glyph_set_id, &req.pages, &req.glyphs, |bitmap| {
        shifted(bitmap, req.dx, req.dy, req.overflow)
    })
}

/// Applies `ClearGlyphs`.
pub fn clear_glyphs(doc: &mut FontSpace, req: &ClearGlyphs) -> Result<ChangeSet, FontSpaceError> {
    batch_transform(doc, req.glyph_set_id, &req.pages, &req.glyphs, |bitmap| {
        Bitmap::new_blank(bitmap.size())
    })
}

/// Applies `InvertGlyphs`.
pub fn invert_glyphs(doc: &mut FontSpace, req: &InvertGlyphs) -> Result<ChangeSet, FontSpaceError> {
    batch_transform(doc, req.glyph_set_id, &req.pages, &req.glyphs, inverted)
}

/// The shared engine for the batch glyph transforms: resolve pages and codes, then
/// for each existing glyph among them apply `transform` and record a change when the
/// bitmap actually changed. Absent codes are skipped (transforming a blank is a
/// no-op for these operations). Atomic: nothing mutates until every target resolves.
fn batch_transform(
    doc: &mut FontSpace,
    glyph_set_id: GlyphSetId,
    pages: &PageSelector,
    glyphs: &GlyphSelector,
    transform: impl Fn(&Bitmap) -> Bitmap,
) -> Result<ChangeSet, FontSpaceError> {
    let object_changes = {
        let glyph_set = doc
            .glyph_set(glyph_set_id)
            .ok_or(FontSpaceError::GlyphSetNotFound(glyph_set_id))?;
        let character_set = doc.character_set(glyph_set.character_set_id).ok_or(
            FontSpaceError::CharacterSetNotFound {
                glyph_set: glyph_set_id,
                character_set: glyph_set.character_set_id,
            },
        )?;
        let page_ids = resolve_pages(glyph_set, pages)?;
        let codes = resolve_glyph_codes(character_set, glyphs)?;

        let mut object_changes = Vec::new();
        for &page_id in &page_ids {
            let Some(page) = glyph_set.page_of_id(page_id) else {
                continue; // resolved above, so this never fires
            };
            for &code in &codes {
                let Some(glyph) = page.glyph_of_code(code) else {
                    continue; // absent = blank; nothing to transform
                };
                let before = glyph.bitmap.clone();
                let after = transform(&before);
                if after != before {
                    object_changes.push(ObjectChange::GlyphChanged(GlyphChange {
                        glyph_set_id,
                        page_id,
                        code,
                        before,
                        after,
                    }));
                }
            }
        }
        object_changes
    };

    let change_set = ChangeSet {
        object_changes,
        warnings: Vec::new(),
    };
    apply_change_set(doc, &change_set)?;
    Ok(change_set)
}
