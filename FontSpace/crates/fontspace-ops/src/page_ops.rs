//! Page operations (spec/07 §7.2): add, remove, and reorder pages within a glyph
//! set. Each returns one invertible [`ChangeSet`]; add/remove mint no glyph data,
//! and reorder never touches glyphs.

use fontspace_model::{FontSpace, GlyphPage, GlyphSetId, IdGen, PageId};

use crate::apply_change_set;
use crate::change_set::{ChangeSet, ObjectChange, PageChange, PagesReorder};
use crate::error::FontSpaceError;
use crate::selector::{PageSelector, resolve_pages};
use crate::util::is_permutation;

/// Add a new empty page to a glyph set, at `at_index` (or the end).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddPage {
    pub glyph_set_id: GlyphSetId,
    pub name: String,
    pub description: String,
    pub at_index: Option<usize>,
}

/// Remove the selected pages, with their glyphs (captured for undo).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovePages {
    pub glyph_set_id: GlyphSetId,
    pub pages: PageSelector,
}

/// Reorder a glyph set's pages. `order` must be a permutation of its current page ids.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReorderPages {
    pub glyph_set_id: GlyphSetId,
    pub order: Vec<PageId>,
}

/// Applies `AddPage`, minting a fresh page id from `ids`.
pub fn add_page(
    doc: &mut FontSpace,
    req: &AddPage,
    ids: &mut dyn IdGen,
) -> Result<ChangeSet, FontSpaceError> {
    let index = {
        let glyph_set = doc
            .glyph_set(req.glyph_set_id)
            .ok_or(FontSpaceError::GlyphSetNotFound(req.glyph_set_id))?;
        let len = glyph_set.pages.len();
        let index = req.at_index.unwrap_or(len);
        if index > len {
            return Err(FontSpaceError::PageIndexOutOfRange {
                glyph_set: req.glyph_set_id,
                index,
                len,
            });
        }
        index
    };
    let page = GlyphPage::new(ids, req.name.clone(), req.description.clone());
    let change_set = ChangeSet {
        object_changes: vec![ObjectChange::PageInserted(PageChange {
            glyph_set_id: req.glyph_set_id,
            index,
            page,
        })],
        warnings: Vec::new(),
    };
    apply_change_set(doc, &change_set)?;
    Ok(change_set)
}

/// Applies `RemovePages`.
pub fn remove_pages(doc: &mut FontSpace, req: &RemovePages) -> Result<ChangeSet, FontSpaceError> {
    let object_changes = {
        let glyph_set = doc
            .glyph_set(req.glyph_set_id)
            .ok_or(FontSpaceError::GlyphSetNotFound(req.glyph_set_id))?;
        let page_ids = resolve_pages(glyph_set, &req.pages)?;
        // Record each removal with its index and page (for undo). Descending index
        // order so that forward apply (remove-by-id) and the reversed inverse
        // (insert ascending) both reconstruct positions exactly.
        let mut removals: Vec<(usize, GlyphPage)> = page_ids
            .iter()
            .filter_map(|&id| {
                glyph_set
                    .pages
                    .iter()
                    .position(|page| page.id == id)
                    .map(|index| (index, glyph_set.pages[index].clone()))
            })
            .collect();
        removals.sort_by_key(|(index, _)| std::cmp::Reverse(*index));
        removals
            .into_iter()
            .map(|(index, page)| {
                ObjectChange::PageRemoved(PageChange {
                    glyph_set_id: req.glyph_set_id,
                    index,
                    page,
                })
            })
            .collect()
    };
    let change_set = ChangeSet {
        object_changes,
        warnings: Vec::new(),
    };
    apply_change_set(doc, &change_set)?;
    Ok(change_set)
}

/// Applies `ReorderPages`.
pub fn reorder_pages(doc: &mut FontSpace, req: &ReorderPages) -> Result<ChangeSet, FontSpaceError> {
    let change = {
        let glyph_set = doc
            .glyph_set(req.glyph_set_id)
            .ok_or(FontSpaceError::GlyphSetNotFound(req.glyph_set_id))?;
        let before: Vec<PageId> = glyph_set.pages.iter().map(|page| page.id).collect();
        if !is_permutation(&req.order, &before) {
            return Err(FontSpaceError::InvalidPageOrder {
                glyph_set: req.glyph_set_id,
                expected: before.len(),
            });
        }
        if req.order == before {
            return Ok(ChangeSet::default());
        }
        ObjectChange::PagesReordered(PagesReorder {
            glyph_set_id: req.glyph_set_id,
            before,
            after: req.order.clone(),
        })
    };
    let change_set = ChangeSet {
        object_changes: vec![change],
        warnings: Vec::new(),
    };
    apply_change_set(doc, &change_set)?;
    Ok(change_set)
}
