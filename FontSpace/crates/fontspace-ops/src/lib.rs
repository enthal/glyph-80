#![forbid(unsafe_code)]

//! FontSpace operations (spec/07): selectors, pixel/glyph commands, and invertible
//! change sets. Every operation resolves and validates its targets before any
//! mutation, computes all outputs, applies them atomically, and returns one
//! [`ChangeSet`] — the undo/redo unit. Undo is "apply the inverse"; redo is
//! "apply the change set again".
//!
//! Slice 4a implements the pixel/glyph operations. Page, guide, and character-set
//! operations (including the remove-cascade) arrive in the following slices.

mod change_set;
mod error;
mod glyph_ops;
mod selector;

use fontspace_model::{FontSpace, Glyph};

pub use change_set::{ChangeSet, FontSpaceWarning, GlyphChange, ObjectChange};
pub use error::FontSpaceError;
pub use glyph_ops::{
    ClearGlyphs, GlyphRef, InvertGlyphs, PixelEdit, SetPixels, ShiftGlyphs, clear_glyphs,
    invert_glyphs, set_pixels, shift_glyphs,
};
pub use selector::{GlyphSelector, PageSelector};

/// Applies a change set forward (also the redo primitive). Sets each changed glyph's
/// bitmap to its `after`, materializing the glyph if absent. Fails only if the change
/// set references a glyph set or page not present in `doc`.
///
/// This assumes `change_set` matches the current document — as it does when it came
/// from an operation on this document, or its inverse. It applies changes in order
/// and is **not** self-atomic on a mismatched set; the operation functions guarantee
/// atomicity by resolving and validating every target before calling this.
pub fn apply_change_set(doc: &mut FontSpace, change_set: &ChangeSet) -> Result<(), FontSpaceError> {
    for change in &change_set.object_changes {
        match change {
            ObjectChange::GlyphChanged(glyph_change) => apply_glyph_change(doc, glyph_change)?,
        }
    }
    Ok(())
}

/// Undoes a change set by applying its inverse (spec/07 §7.7).
pub fn undo(doc: &mut FontSpace, change_set: &ChangeSet) -> Result<(), FontSpaceError> {
    apply_change_set(doc, &change_set.invert())
}

fn apply_glyph_change(doc: &mut FontSpace, change: &GlyphChange) -> Result<(), FontSpaceError> {
    let glyph_set = doc
        .glyph_sets
        .iter_mut()
        .find(|glyph_set| glyph_set.id == change.glyph_set_id)
        .ok_or(FontSpaceError::GlyphSetNotFound(change.glyph_set_id))?;
    let page = glyph_set
        .pages
        .iter_mut()
        .find(|page| page.id == change.page_id)
        .ok_or(FontSpaceError::PageIdNotFound {
            glyph_set: change.glyph_set_id,
            page: change.page_id,
        })?;
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

#[cfg(test)]
mod tests;
