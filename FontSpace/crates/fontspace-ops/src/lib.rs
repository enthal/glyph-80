#![forbid(unsafe_code)]

//! FontSpace operations (spec/07): selectors, commands, and invertible change sets.
//! Every operation resolves and validates its targets before any mutation, computes
//! all outputs, applies them atomically, and returns one [`ChangeSet`] — the
//! undo/redo unit. Undo is "apply the inverse"; redo is "apply the change set again".
//!
//! This covers the full Milestone-1 operation set: glyph, page, guide, and
//! character-set edits — including the remove-entry cascade that deletes referencing
//! glyphs across the document, atomically and invertibly.

mod change_set;
mod char_set_ops;
mod error;
mod glyph_ops;
mod guide_ops;
mod page_ops;
mod selector;
mod util;

use std::collections::HashMap;

use fontspace_model::{
    CharacterSet, CharacterSetId, FontSpace, Glyph, GlyphPage, GlyphSet, GlyphSetId, PageId,
};

pub use change_set::{
    CascadedGlyph, ChangeSet, CharacterSetChange, FontSpaceWarning, GlyphChange, GlyphPlacement,
    GuideChange, ObjectChange, OrphanedGlyph, PageChange, PagesReorder,
};
pub use char_set_ops::{
    AddCharacterEntry, RecodeCharacterEntry, RemoveCharacterEntry, ReorderCharacterEntries,
    add_character_entry, recode_character_entry, remove_character_entry, reorder_character_entries,
};
pub use error::FontSpaceError;
pub use glyph_ops::{
    ClearGlyphs, GlyphRef, InvertGlyphs, PixelEdit, SetPixels, ShiftGlyphs, clear_glyphs,
    invert_glyphs, set_pixels, shift_glyphs,
};
pub use guide_ops::{
    AddGuide, CopyGuideToPages, MoveGuide, RemoveGuide, SetGuideVisible, add_guide,
    copy_guide_to_pages, move_guide, remove_guide, set_guide_visible,
};
pub use page_ops::{AddPage, RemovePages, ReorderPages, add_page, remove_pages, reorder_pages};
pub use selector::{GlyphSelector, PageSelector, resolve_glyph_codes_in, resolve_pages_in};

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
            ObjectChange::CharacterSetChanged(change) => apply_character_set_change(doc, change)?,
            ObjectChange::GlyphRemoved(placement) => apply_glyph_remove(doc, placement)?,
            ObjectChange::GlyphInserted(placement) => apply_glyph_insert(doc, placement)?,
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

/// Removes the (single) glyph for `placement.code` from its page.
fn apply_glyph_remove(
    doc: &mut FontSpace,
    placement: &GlyphPlacement,
) -> Result<(), FontSpaceError> {
    let glyph_set = glyph_set_mut(doc, placement.glyph_set_id)?;
    let page = page_mut(glyph_set, placement.glyph_set_id, placement.page_id)?;
    page.glyphs.retain(|glyph| glyph.code != placement.code);
    Ok(())
}

/// Re-inserts a whole glyph at its recorded index (the inverse of a removal).
fn apply_glyph_insert(
    doc: &mut FontSpace,
    placement: &GlyphPlacement,
) -> Result<(), FontSpaceError> {
    let glyph_set = glyph_set_mut(doc, placement.glyph_set_id)?;
    let page = page_mut(glyph_set, placement.glyph_set_id, placement.page_id)?;
    let index = placement.index.min(page.glyphs.len());
    page.glyphs.insert(
        index,
        Glyph {
            code: placement.code,
            bitmap: placement.bitmap.clone(),
        },
    );
    Ok(())
}

fn character_set_mut(
    doc: &mut FontSpace,
    id: CharacterSetId,
) -> Result<&mut CharacterSet, FontSpaceError> {
    doc.character_sets
        .iter_mut()
        .find(|character_set| character_set.id == id)
        .ok_or(FontSpaceError::CharacterSetIdNotFound(id))
}

fn apply_character_set_change(
    doc: &mut FontSpace,
    change: &CharacterSetChange,
) -> Result<(), FontSpaceError> {
    let character_set = character_set_mut(doc, change.character_set_id)?;
    character_set.entries = change.after.clone();
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
