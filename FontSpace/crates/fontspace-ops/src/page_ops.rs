//! Page operations (spec/07 §7.2): add, remove, reorder, rename, duplicate, and move
//! pages within (and between) glyph sets. Each returns one invertible [`ChangeSet`];
//! add/remove/reorder mint no glyph data. Rename and move are modelled as a
//! remove-then-insert pair over the existing [`PageChange`] machinery (the page keeps
//! its id), so they need no new change variant.

use fontspace_model::{FontSpace, GlyphPage, GlyphSetId, GuideId, IdGen, PageId};

use crate::apply_change_set;
use crate::change_set::{ChangeSet, ObjectChange, PageChange, PagesReorder};
use crate::error::FontSpaceError;
use crate::selector::{PageSelector, resolve_pages};
use crate::util::is_permutation;

/// The `(index, page-clone)` for `page_id` in glyph set `glyph_set_id`, or a not-found
/// error — the resolve step shared by rename/duplicate/move.
fn locate_page(
    doc: &FontSpace,
    glyph_set_id: GlyphSetId,
    page_id: PageId,
) -> Result<(usize, GlyphPage), FontSpaceError> {
    let glyph_set = doc
        .glyph_set(glyph_set_id)
        .ok_or(FontSpaceError::GlyphSetNotFound(glyph_set_id))?;
    glyph_set
        .pages
        .iter()
        .position(|page| page.id == page_id)
        .map(|index| (index, glyph_set.pages[index].clone()))
        .ok_or(FontSpaceError::PageIdNotFound {
            glyph_set: glyph_set_id,
            page: page_id,
        })
}

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

/// Rename a page in place (spec/07 §7.2). A no-op when the name is unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenamePage {
    pub glyph_set_id: GlyphSetId,
    pub page_id: PageId,
    pub name: String,
}

/// Duplicate a page within its glyph set — a copy with a **fresh** page id (and fresh
/// guide ids), inserted right after the original; its glyphs copy verbatim (keyed by
/// `code`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicatePage {
    pub glyph_set_id: GlyphSetId,
    pub page_id: PageId,
    pub name: String,
}

/// Move a page from one glyph set to another (or reposition within one). The page keeps
/// its id and glyphs; both glyph sets must share the same geometry (spec/03 §3.5).
/// `at_index` is the destination position (or the end when `None`). Glyphs whose `code`
/// has no entry in the destination's character set become tolerated dangling glyphs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovePage {
    pub from_glyph_set_id: GlyphSetId,
    pub page_id: PageId,
    pub to_glyph_set_id: GlyphSetId,
    pub at_index: Option<usize>,
}

/// Applies `RenamePage` as a remove+insert of the same-id page (the page keeps its id,
/// so undo is exact).
pub fn rename_page(doc: &mut FontSpace, req: &RenamePage) -> Result<ChangeSet, FontSpaceError> {
    let (index, page) = locate_page(doc, req.glyph_set_id, req.page_id)?;
    if page.name == req.name {
        return Ok(ChangeSet::default());
    }
    let mut renamed = page.clone();
    renamed.name = req.name.clone();
    let change_set = ChangeSet {
        object_changes: vec![
            ObjectChange::PageRemoved(PageChange {
                glyph_set_id: req.glyph_set_id,
                index,
                page,
            }),
            ObjectChange::PageInserted(PageChange {
                glyph_set_id: req.glyph_set_id,
                index,
                page: renamed,
            }),
        ],
        warnings: Vec::new(),
    };
    apply_change_set(doc, &change_set)?;
    Ok(change_set)
}

/// Applies `DuplicatePage`, minting a fresh page id (and guide ids) from `ids`.
pub fn duplicate_page(
    doc: &mut FontSpace,
    req: &DuplicatePage,
    ids: &mut dyn IdGen,
) -> Result<ChangeSet, FontSpaceError> {
    let (index, mut copy) = locate_page(doc, req.glyph_set_id, req.page_id)?;
    copy.id = PageId::new(ids);
    copy.name = req.name.clone();
    for guide in &mut copy.guides {
        guide.id = GuideId::new(ids);
    }
    let change_set = ChangeSet {
        object_changes: vec![ObjectChange::PageInserted(PageChange {
            glyph_set_id: req.glyph_set_id,
            index: index + 1,
            page: copy,
        })],
        warnings: Vec::new(),
    };
    apply_change_set(doc, &change_set)?;
    Ok(change_set)
}

/// Applies `MovePage` as a remove-from-source + insert-into-destination. Rejects a
/// geometry mismatch between the two glyph sets before mutating (atomic, spec/07 §7.6).
pub fn move_page(doc: &mut FontSpace, req: &MovePage) -> Result<ChangeSet, FontSpaceError> {
    let (from_index, page) = locate_page(doc, req.from_glyph_set_id, req.page_id)?;
    let (source_size, target_size, to_len) = {
        let from = doc
            .glyph_set(req.from_glyph_set_id)
            .ok_or(FontSpaceError::GlyphSetNotFound(req.from_glyph_set_id))?;
        let to = doc
            .glyph_set(req.to_glyph_set_id)
            .ok_or(FontSpaceError::GlyphSetNotFound(req.to_glyph_set_id))?;
        (from.glyph_size, to.glyph_size, to.pages.len())
    };
    if source_size != target_size {
        return Err(FontSpaceError::GeometryMismatch {
            glyph_set: req.to_glyph_set_id,
            page: req.page_id,
            source_size,
            target_size,
        });
    }
    // Destination index: within the target after the source is removed. When moving
    // within one glyph set the removal shifts later indices down by one.
    let same_set = req.from_glyph_set_id == req.to_glyph_set_id;
    let max_index = if same_set { to_len - 1 } else { to_len };
    let to_index = req.at_index.unwrap_or(max_index).min(max_index);
    let change_set = ChangeSet {
        object_changes: vec![
            ObjectChange::PageRemoved(PageChange {
                glyph_set_id: req.from_glyph_set_id,
                index: from_index,
                page: page.clone(),
            }),
            ObjectChange::PageInserted(PageChange {
                glyph_set_id: req.to_glyph_set_id,
                index: to_index,
                page,
            }),
        ],
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
