#![forbid(unsafe_code)]

//! FontSpace operations (spec/07): selectors, commands, and invertible change sets.
//! Every operation resolves and validates its targets before any mutation, computes
//! all outputs, applies them atomically, and returns one [`ChangeSet`] — the
//! undo/redo unit. Undo is "apply the inverse"; redo is "apply the change set again".
//!
//! Slices 4a–4b implement the glyph, page, and guide operations. The character-set
//! edit operations (including the remove-cascade) arrive in the next slice.

mod change_set;
mod error;
mod glyph_ops;
mod guide_ops;
mod page_ops;
mod selector;

use std::collections::HashMap;

use fontspace_model::{FontSpace, Glyph, GlyphPage, GlyphSet, GlyphSetId, PageId};

pub use change_set::{
    ChangeSet, FontSpaceWarning, GlyphChange, GuideChange, ObjectChange, PageChange, PagesReorder,
};
pub use error::FontSpaceError;
pub use glyph_ops::{
    ClearGlyphs, GlyphRef, InvertGlyphs, PixelEdit, SetPixels, ShiftGlyphs, clear_glyphs,
    invert_glyphs, set_pixels, shift_glyphs,
};
pub use guide_ops::{
    AddGuide, CopyGuideToPages, MoveGuide, add_guide, copy_guide_to_pages, move_guide,
};
pub use page_ops::{AddPage, RemovePages, ReorderPages, add_page, remove_pages, reorder_pages};
pub use selector::{GlyphSelector, PageSelector};

/// Applies a change set forward (also the redo primitive).
///
/// This assumes `change_set` matches the current document — as it does when it came
/// from an operation on this document, or its inverse. It applies changes in order
/// and is **not** self-atomic on a mismatched set; the operation functions guarantee
/// atomicity by resolving and validating every target before calling this.
pub fn apply_change_set(doc: &mut FontSpace, change_set: &ChangeSet) -> Result<(), FontSpaceError> {
    for change in &change_set.object_changes {
        match change {
            ObjectChange::GlyphChanged(glyph_change) => apply_glyph_change(doc, glyph_change)?,
            ObjectChange::PageInserted(page_change) => apply_page_insert(doc, page_change)?,
            ObjectChange::PageRemoved(page_change) => apply_page_remove(doc, page_change)?,
            ObjectChange::PagesReordered(reorder) => apply_pages_reorder(doc, reorder)?,
            ObjectChange::GuideChanged(guide_change) => apply_guide_change(doc, guide_change)?,
        }
    }
    Ok(())
}

/// Undoes a change set by applying its inverse (spec/07 §7.7).
pub fn undo(doc: &mut FontSpace, change_set: &ChangeSet) -> Result<(), FontSpaceError> {
    apply_change_set(doc, &change_set.invert())
}

fn glyph_set_mut(doc: &mut FontSpace, id: GlyphSetId) -> Result<&mut GlyphSet, FontSpaceError> {
    doc.glyph_sets
        .iter_mut()
        .find(|glyph_set| glyph_set.id == id)
        .ok_or(FontSpaceError::GlyphSetNotFound(id))
}

fn page_mut(
    glyph_set: &mut GlyphSet,
    glyph_set_id: GlyphSetId,
    page_id: PageId,
) -> Result<&mut GlyphPage, FontSpaceError> {
    glyph_set
        .pages
        .iter_mut()
        .find(|page| page.id == page_id)
        .ok_or(FontSpaceError::PageIdNotFound {
            glyph_set: glyph_set_id,
            page: page_id,
        })
}

/// Sets a glyph's bitmap to `after`, materializing the glyph if absent.
fn apply_glyph_change(doc: &mut FontSpace, change: &GlyphChange) -> Result<(), FontSpaceError> {
    let glyph_set = glyph_set_mut(doc, change.glyph_set_id)?;
    let page = page_mut(glyph_set, change.glyph_set_id, change.page_id)?;
    match page
        .glyphs
        .iter_mut()
        .find(|glyph| glyph.code == change.code)
    {
        Some(glyph) => glyph.bitmap = change.after.clone(),
        None => page.glyphs.push(Glyph {
            code: change.code,
            bitmap: change.after.clone(),
        }),
    }
    Ok(())
}

fn apply_page_insert(doc: &mut FontSpace, change: &PageChange) -> Result<(), FontSpaceError> {
    let glyph_set = glyph_set_mut(doc, change.glyph_set_id)?;
    let index = change.index.min(glyph_set.pages.len());
    glyph_set.pages.insert(index, change.page.clone());
    Ok(())
}

fn apply_page_remove(doc: &mut FontSpace, change: &PageChange) -> Result<(), FontSpaceError> {
    // Remove by id so a batch of removals is order-independent (spec/07 §7.7).
    let glyph_set = glyph_set_mut(doc, change.glyph_set_id)?;
    glyph_set.pages.retain(|page| page.id != change.page.id);
    Ok(())
}

fn apply_pages_reorder(doc: &mut FontSpace, change: &PagesReorder) -> Result<(), FontSpaceError> {
    let glyph_set = glyph_set_mut(doc, change.glyph_set_id)?;
    let mut pages_by_id: HashMap<PageId, GlyphPage> = glyph_set
        .pages
        .drain(..)
        .map(|page| (page.id, page))
        .collect();
    glyph_set.pages = change
        .after
        .iter()
        .filter_map(|id| pages_by_id.remove(id))
        .collect();
    Ok(())
}

fn apply_guide_change(doc: &mut FontSpace, change: &GuideChange) -> Result<(), FontSpaceError> {
    let glyph_set = glyph_set_mut(doc, change.glyph_set_id)?;
    let page = page_mut(glyph_set, change.glyph_set_id, change.page_id)?;
    match &change.after {
        Some(guide) => match page.guides.iter_mut().find(|g| g.id == change.guide_id) {
            Some(existing) => *existing = guide.clone(),
            None => page.guides.push(guide.clone()),
        },
        None => page.guides.retain(|g| g.id != change.guide_id),
    }
    Ok(())
}

#[cfg(test)]
mod tests;
