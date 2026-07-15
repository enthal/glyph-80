//! Guide operations (spec/07 §7.2): add, move, remove, set-visible, and
//! copy-to-pages. Every guide change carries the guide's index so remove's inverse
//! re-inserts at its position (spec/07 §7.7). Copying mints a fresh `GuideId` per
//! target page so ids never collide across pages (spec/03 §3.8).

use fontspace_model::{FontSpace, GlyphSetId, Guide, GuideAxis, GuideId, IdGen, PageId};

use crate::apply_change_set;
use crate::change_set::{ChangeSet, GuideChange, ObjectChange};
use crate::error::FontSpaceError;
use crate::selector::{PageSelector, resolve_pages};

/// Add a guide to a page, minting a fresh id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddGuide {
    pub glyph_set_id: GlyphSetId,
    pub page_id: PageId,
    pub name: String,
    pub axis: GuideAxis,
    pub position: i32,
    pub visible: bool,
    pub locked: bool,
}

/// Move an existing guide to a new position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveGuide {
    pub glyph_set_id: GlyphSetId,
    pub page_id: PageId,
    pub guide_id: GuideId,
    pub position: i32,
}

/// Rename an existing guide. A no-op if the name is unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameGuide {
    pub glyph_set_id: GlyphSetId,
    pub page_id: PageId,
    pub guide_id: GuideId,
    pub name: String,
}

/// Remove a guide from a page. Invertible: undo re-inserts the same guide (spec/07
/// §7.7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoveGuide {
    pub glyph_set_id: GlyphSetId,
    pub page_id: PageId,
    pub guide_id: GuideId,
}

/// Set a guide's visibility (show/hide). A no-op if already in that state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetGuideVisible {
    pub glyph_set_id: GlyphSetId,
    pub page_id: PageId,
    pub guide_id: GuideId,
    pub visible: bool,
}

/// Copy one guide onto other pages, minting a fresh id on each target (the source
/// page is skipped). Guide identity never collides across pages (spec/03 §3.8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyGuideToPages {
    pub glyph_set_id: GlyphSetId,
    pub source_page_id: PageId,
    pub guide_id: GuideId,
    pub target_pages: PageSelector,
}

/// Applies `AddGuide`. The guide is appended, so its index is the current count.
pub fn add_guide(
    doc: &mut FontSpace,
    req: &AddGuide,
    ids: &mut dyn IdGen,
) -> Result<ChangeSet, FontSpaceError> {
    let index = {
        let glyph_set = doc
            .glyph_set(req.glyph_set_id)
            .ok_or(FontSpaceError::GlyphSetNotFound(req.glyph_set_id))?;
        let page = glyph_set
            .page_of_id(req.page_id)
            .ok_or(FontSpaceError::PageIdNotFound {
                glyph_set: req.glyph_set_id,
                page: req.page_id,
            })?;
        page.guides.len()
    };
    let guide = Guide {
        id: GuideId::new(ids),
        name: req.name.clone(),
        axis: req.axis,
        position: req.position,
        visible: req.visible,
        locked: req.locked,
    };
    let change_set = ChangeSet {
        object_changes: vec![ObjectChange::GuideChanged(GuideChange {
            glyph_set_id: req.glyph_set_id,
            page_id: req.page_id,
            guide_id: guide.id,
            index,
            before: None,
            after: Some(guide),
        })],
        warnings: Vec::new(),
    };
    apply_change_set(doc, &change_set)?;
    Ok(change_set)
}

/// Applies `MoveGuide`.
pub fn move_guide(doc: &mut FontSpace, req: &MoveGuide) -> Result<ChangeSet, FontSpaceError> {
    let change = {
        let (index, guide) = find_guide(doc, req.glyph_set_id, req.page_id, req.guide_id)?;
        if guide.position == req.position {
            return Ok(ChangeSet::default());
        }
        let before = guide.clone();
        let mut after = guide.clone();
        after.position = req.position;
        ObjectChange::GuideChanged(GuideChange {
            glyph_set_id: req.glyph_set_id,
            page_id: req.page_id,
            guide_id: req.guide_id,
            index,
            before: Some(before),
            after: Some(after),
        })
    };
    let change_set = ChangeSet {
        object_changes: vec![change],
        warnings: Vec::new(),
    };
    apply_change_set(doc, &change_set)?;
    Ok(change_set)
}

/// Applies `RenameGuide`.
pub fn rename_guide(doc: &mut FontSpace, req: &RenameGuide) -> Result<ChangeSet, FontSpaceError> {
    let change = {
        let (index, guide) = find_guide(doc, req.glyph_set_id, req.page_id, req.guide_id)?;
        if guide.name == req.name {
            return Ok(ChangeSet::default());
        }
        let before = guide.clone();
        let mut after = guide.clone();
        after.name = req.name.clone();
        ObjectChange::GuideChanged(GuideChange {
            glyph_set_id: req.glyph_set_id,
            page_id: req.page_id,
            guide_id: req.guide_id,
            index,
            before: Some(before),
            after: Some(after),
        })
    };
    let change_set = ChangeSet {
        object_changes: vec![change],
        warnings: Vec::new(),
    };
    apply_change_set(doc, &change_set)?;
    Ok(change_set)
}

/// Applies `RemoveGuide`.
pub fn remove_guide(doc: &mut FontSpace, req: &RemoveGuide) -> Result<ChangeSet, FontSpaceError> {
    let (index, guide) = find_guide(doc, req.glyph_set_id, req.page_id, req.guide_id)?;
    let before = guide.clone();
    let change_set = ChangeSet {
        object_changes: vec![ObjectChange::GuideChanged(GuideChange {
            glyph_set_id: req.glyph_set_id,
            page_id: req.page_id,
            guide_id: req.guide_id,
            index,
            before: Some(before),
            after: None,
        })],
        warnings: Vec::new(),
    };
    apply_change_set(doc, &change_set)?;
    Ok(change_set)
}

/// Applies `SetGuideVisible`.
pub fn set_guide_visible(
    doc: &mut FontSpace,
    req: &SetGuideVisible,
) -> Result<ChangeSet, FontSpaceError> {
    let change = {
        let (index, guide) = find_guide(doc, req.glyph_set_id, req.page_id, req.guide_id)?;
        if guide.visible == req.visible {
            return Ok(ChangeSet::default());
        }
        let before = guide.clone();
        let mut after = guide.clone();
        after.visible = req.visible;
        ObjectChange::GuideChanged(GuideChange {
            glyph_set_id: req.glyph_set_id,
            page_id: req.page_id,
            guide_id: req.guide_id,
            index,
            before: Some(before),
            after: Some(after),
        })
    };
    let change_set = ChangeSet {
        object_changes: vec![change],
        warnings: Vec::new(),
    };
    apply_change_set(doc, &change_set)?;
    Ok(change_set)
}

/// Applies `CopyGuideToPages`.
pub fn copy_guide_to_pages(
    doc: &mut FontSpace,
    req: &CopyGuideToPages,
    ids: &mut dyn IdGen,
) -> Result<ChangeSet, FontSpaceError> {
    let source = find_guide(doc, req.glyph_set_id, req.source_page_id, req.guide_id)?
        .1
        .clone();
    let target_page_ids = {
        let glyph_set = doc
            .glyph_set(req.glyph_set_id)
            .ok_or(FontSpaceError::GlyphSetNotFound(req.glyph_set_id))?;
        resolve_pages(glyph_set, &req.target_pages)?
    };
    let glyph_set = doc
        .glyph_set(req.glyph_set_id)
        .ok_or(FontSpaceError::GlyphSetNotFound(req.glyph_set_id))?;
    let mut object_changes = Vec::new();
    for page_id in target_page_ids {
        if page_id == req.source_page_id {
            continue; // copying onto the source page would just duplicate it
        }
        // Each copy is appended to its target page, so its index is that page's
        // current guide count (each target page is distinct, so this is stable).
        let index = glyph_set
            .page_of_id(page_id)
            .map_or(0, |page| page.guides.len());
        let copy = Guide {
            id: GuideId::new(ids),
            name: source.name.clone(),
            axis: source.axis,
            position: source.position,
            visible: source.visible,
            locked: source.locked,
        };
        object_changes.push(ObjectChange::GuideChanged(GuideChange {
            glyph_set_id: req.glyph_set_id,
            page_id,
            guide_id: copy.id,
            index,
            before: None,
            after: Some(copy),
        }));
    }
    let change_set = ChangeSet {
        object_changes,
        warnings: Vec::new(),
    };
    apply_change_set(doc, &change_set)?;
    Ok(change_set)
}

/// Finds a guide and its index within the page's `guides` vector.
fn find_guide(
    doc: &FontSpace,
    glyph_set_id: GlyphSetId,
    page_id: PageId,
    guide_id: GuideId,
) -> Result<(usize, &Guide), FontSpaceError> {
    let glyph_set = doc
        .glyph_set(glyph_set_id)
        .ok_or(FontSpaceError::GlyphSetNotFound(glyph_set_id))?;
    let page = glyph_set
        .page_of_id(page_id)
        .ok_or(FontSpaceError::PageIdNotFound {
            glyph_set: glyph_set_id,
            page: page_id,
        })?;
    page.guides
        .iter()
        .enumerate()
        .find(|(_, guide)| guide.id == guide_id)
        .ok_or(FontSpaceError::GuideNotFound {
            glyph_set: glyph_set_id,
            page: page_id,
            guide: guide_id,
        })
}
