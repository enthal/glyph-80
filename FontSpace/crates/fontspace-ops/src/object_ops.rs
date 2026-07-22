//! Top-level document-object operations (spec/07 §7.2): adding, renaming, duplicating,
//! and removing a glyph set, and adding/replacing an export config. Each returns one
//! invertible [`ChangeSet`] — the same undo/redo unit as every other operation — so a
//! create, rename, or delete undoes cleanly.
//!
//! The GUI and CLI *construct* the object (a glyph set of a chosen geometry, an export
//! config built from a scan preset in `fontspace-export`) and hand it here; these ops
//! own only the document-level insert/replace and its invertible record (CLAUDE.md:
//! UI layers construct and invoke domain operations).

use fontspace_model::{
    CharacterSetId, ExportConfig, FontSpace, GlyphPage, GlyphSet, GlyphSetId, GlyphSize, GuideId,
    IdGen, PageId,
};

use crate::apply_change_set;
use crate::change_set::{ChangeSet, ExportConfigChange, GlyphSetChange, ObjectChange};
use crate::error::FontSpaceError;

/// Add a new glyph set to the document, referencing an existing character set. When
/// `initial_page_name` is `Some`, the set is created with one empty page of that name
/// so it is immediately editable; `None` creates a pageless set (spec/03 §3.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddGlyphSet {
    pub name: String,
    pub description: String,
    pub glyph_size: GlyphSize,
    pub character_set_id: CharacterSetId,
    pub initial_page_name: Option<String>,
}

/// Applies [`AddGlyphSet`], minting a fresh glyph-set id (and page id) from `ids`. The
/// referenced character set must exist — a glyph set that dangles its character set is
/// rejected up front (atomicity, spec/07 §7.6).
pub fn add_glyph_set(
    doc: &mut FontSpace,
    req: &AddGlyphSet,
    ids: &mut dyn IdGen,
) -> Result<ChangeSet, FontSpaceError> {
    if doc.character_set(req.character_set_id).is_none() {
        return Err(FontSpaceError::CharacterSetIdNotFound(req.character_set_id));
    }
    let mut glyph_set = GlyphSet::new(
        ids,
        req.name.clone(),
        req.description.clone(),
        req.glyph_size,
        req.character_set_id,
    );
    if let Some(page_name) = &req.initial_page_name {
        glyph_set
            .pages
            .push(GlyphPage::new(ids, page_name.clone(), ""));
    }
    let change_set = ChangeSet {
        object_changes: vec![ObjectChange::GlyphSetChanged(Box::new(GlyphSetChange {
            index: doc.glyph_sets.len(),
            before: None,
            after: Some(glyph_set),
        }))],
        warnings: Vec::new(),
    };
    apply_change_set(doc, &change_set)?;
    Ok(change_set)
}

/// The index of the glyph set with `id`, or a not-found error.
fn glyph_set_index(doc: &FontSpace, id: GlyphSetId) -> Result<usize, FontSpaceError> {
    doc.glyph_sets
        .iter()
        .position(|gs| gs.id == id)
        .ok_or(FontSpaceError::GlyphSetNotFound(id))
}

/// Rename a glyph set (spec/07 §7.2). A no-op (empty change set) when unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameGlyphSet {
    pub glyph_set_id: GlyphSetId,
    pub name: String,
}

/// Applies [`RenameGlyphSet`], recording the prior set for undo.
pub fn rename_glyph_set(
    doc: &mut FontSpace,
    req: &RenameGlyphSet,
) -> Result<ChangeSet, FontSpaceError> {
    let index = glyph_set_index(doc, req.glyph_set_id)?;
    if doc.glyph_sets[index].name == req.name {
        return Ok(ChangeSet::default());
    }
    let before = doc.glyph_sets[index].clone();
    let mut after = before.clone();
    after.name = req.name.clone();
    let change_set = ChangeSet {
        object_changes: vec![ObjectChange::GlyphSetChanged(Box::new(GlyphSetChange {
            index,
            before: Some(before),
            after: Some(after),
        }))],
        warnings: Vec::new(),
    };
    apply_change_set(doc, &change_set)?;
    Ok(change_set)
}

/// Remove a glyph set and everything under it (spec/07 §7.2); undo restores it whole. An
/// export config that sourced it is left in place and fails validation until repointed or
/// removed (a dangling source, surfaced at export time — not a load error).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoveGlyphSet {
    pub glyph_set_id: GlyphSetId,
}

/// Applies [`RemoveGlyphSet`], capturing the removed set (at its index) for undo.
pub fn remove_glyph_set(
    doc: &mut FontSpace,
    req: &RemoveGlyphSet,
) -> Result<ChangeSet, FontSpaceError> {
    let index = glyph_set_index(doc, req.glyph_set_id)?;
    let before = doc.glyph_sets[index].clone();
    let change_set = ChangeSet {
        object_changes: vec![ObjectChange::GlyphSetChanged(Box::new(GlyphSetChange {
            index,
            before: Some(before),
            after: None,
        }))],
        warnings: Vec::new(),
    };
    apply_change_set(doc, &change_set)?;
    Ok(change_set)
}

/// Duplicate a glyph set — a deep copy with **fresh** ids (the glyph set, its pages, and
/// their guides), inserted right after the original (spec/07 §7.2). Glyphs and character
/// entries are keyed by `code`, so they copy verbatim, and the copy references the **same**
/// character set (spec/03 §3.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateGlyphSet {
    pub glyph_set_id: GlyphSetId,
    pub name: String,
}

/// Applies [`DuplicateGlyphSet`], minting fresh ids from `ids`.
pub fn duplicate_glyph_set(
    doc: &mut FontSpace,
    req: &DuplicateGlyphSet,
    ids: &mut dyn IdGen,
) -> Result<ChangeSet, FontSpaceError> {
    let index = glyph_set_index(doc, req.glyph_set_id)?;
    let mut copy = doc.glyph_sets[index].clone();
    copy.id = GlyphSetId::new(ids);
    copy.name = req.name.clone();
    for page in &mut copy.pages {
        page.id = PageId::new(ids);
        for guide in &mut page.guides {
            guide.id = GuideId::new(ids);
        }
    }
    let change_set = ChangeSet {
        object_changes: vec![ObjectChange::GlyphSetChanged(Box::new(GlyphSetChange {
            index: index + 1,
            before: None,
            after: Some(copy),
        }))],
        warnings: Vec::new(),
    };
    apply_change_set(doc, &change_set)?;
    Ok(change_set)
}

/// Add an already-constructed export config to the document (spec/07 §7.2, spec/10).
/// The caller builds the config — e.g. via a `fontspace-export` scan preset — so this
/// op owns only the document-level insert and its invertible record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddExportConfig {
    pub config: ExportConfig,
}

/// Applies [`AddExportConfig`], appending the config to the document's export configs.
pub fn add_export_config(
    doc: &mut FontSpace,
    req: &AddExportConfig,
) -> Result<ChangeSet, FontSpaceError> {
    let change_set = ChangeSet {
        object_changes: vec![ObjectChange::ExportConfigChanged(Box::new(
            ExportConfigChange {
                index: doc.export_configs.len(),
                before: None,
                after: Some(req.config.clone()),
            },
        ))],
        warnings: Vec::new(),
    };
    apply_change_set(doc, &change_set)?;
    Ok(change_set)
}

/// Replace an existing export config in place, matched by its `id` (spec/07 §7.2). The
/// whole config is swapped — the export editor rebuilds it from its high-level
/// parameters and hands the result here — so one edit is one undo entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplaceExportConfig {
    pub config: ExportConfig,
}

/// Applies [`ReplaceExportConfig`], recording the prior config for undo. Errors if no
/// config with the given id is present.
pub fn replace_export_config(
    doc: &mut FontSpace,
    req: &ReplaceExportConfig,
) -> Result<ChangeSet, FontSpaceError> {
    let index = doc
        .export_configs
        .iter()
        .position(|config| config.id == req.config.id)
        .ok_or(FontSpaceError::ExportConfigNotFound(req.config.id))?;
    let change_set = ChangeSet {
        object_changes: vec![ObjectChange::ExportConfigChanged(Box::new(
            ExportConfigChange {
                index,
                before: Some(doc.export_configs[index].clone()),
                after: Some(req.config.clone()),
            },
        ))],
        warnings: Vec::new(),
    };
    apply_change_set(doc, &change_set)?;
    Ok(change_set)
}
