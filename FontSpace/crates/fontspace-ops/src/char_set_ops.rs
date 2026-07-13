//! Character-set edit operations that touch only the entry list (spec/04 §4.4,
//! spec/07 §7.2): add, reorder, and recode entries. None of these move glyph data —
//! the whole point of `code`-as-identity is that reorder/add/relabel never cascade,
//! and recode only **warns** about glyphs it orphans (spec/04 §4.1). The one edit
//! that does cascade — remove entry — lands in the next slice.

use std::collections::HashMap;

use fontspace_model::{CharacterEntry, CharacterSetId, FontSpace};

use crate::apply_change_set;
use crate::change_set::{
    CascadedGlyph, ChangeSet, CharacterSetChange, FontSpaceWarning, GlyphPlacement, ObjectChange,
    OrphanedGlyph,
};
use crate::error::FontSpaceError;
use crate::util::is_permutation;

/// Add an entry to a character set at `at_index` (or the end). No glyph changes; the
/// new code renders blank until a glyph is drawn (spec/04 §4.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddCharacterEntry {
    pub character_set_id: CharacterSetId,
    pub code: u32,
    pub label: String,
    pub at_index: Option<usize>,
}

/// Reorder a character set's entries. `order` must be a permutation of its current
/// entry codes. Changes display and canonical storage order; touches no glyphs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReorderCharacterEntries {
    pub character_set_id: CharacterSetId,
    pub order: Vec<u32>,
}

/// Change an entry's `code` from `from_code` to `to_code` — "this slot is now a
/// different character". Moves no glyph data; glyphs still referencing `from_code`
/// become dangling and are reported as a warning (spec/04 §4.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecodeCharacterEntry {
    pub character_set_id: CharacterSetId,
    pub from_code: u32,
    pub to_code: u32,
}

/// Remove an entry and **cascade-delete** every glyph with that code across all
/// pages of every glyph set referencing this character set (spec/04 §4.4, spec/07
/// §7.8). Atomic and invertible; the removed glyphs are listed in a warning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoveCharacterEntry {
    pub character_set_id: CharacterSetId,
    pub code: u32,
}

/// Applies `AddCharacterEntry`.
pub fn add_character_entry(
    doc: &mut FontSpace,
    req: &AddCharacterEntry,
) -> Result<ChangeSet, FontSpaceError> {
    let change = {
        let character_set = doc
            .character_set(req.character_set_id)
            .ok_or(FontSpaceError::CharacterSetIdNotFound(req.character_set_id))?;
        if character_set.contains_code(req.code) {
            return Err(FontSpaceError::DuplicateEntryCode {
                character_set: req.character_set_id,
                code: req.code,
            });
        }
        let before = character_set.entries.clone();
        let index = req.at_index.unwrap_or(before.len());
        if index > before.len() {
            return Err(FontSpaceError::EntryIndexOutOfRange {
                character_set: req.character_set_id,
                index,
                len: before.len(),
            });
        }
        let mut after = before.clone();
        after.insert(
            index,
            CharacterEntry {
                code: req.code,
                label: req.label.clone(),
            },
        );
        ObjectChange::CharacterSetChanged(CharacterSetChange {
            character_set_id: req.character_set_id,
            before,
            after,
        })
    };
    apply_single(doc, change)
}

/// Applies `ReorderCharacterEntries`.
pub fn reorder_character_entries(
    doc: &mut FontSpace,
    req: &ReorderCharacterEntries,
) -> Result<ChangeSet, FontSpaceError> {
    let change = {
        let character_set = doc
            .character_set(req.character_set_id)
            .ok_or(FontSpaceError::CharacterSetIdNotFound(req.character_set_id))?;
        let before = character_set.entries.clone();
        let current: Vec<u32> = before.iter().map(|entry| entry.code).collect();
        if !is_permutation(&req.order, &current) {
            return Err(FontSpaceError::InvalidEntryOrder {
                character_set: req.character_set_id,
                expected: before.len(),
            });
        }
        if req.order == current {
            return Ok(ChangeSet::default());
        }
        let mut entries_by_code: HashMap<u32, CharacterEntry> = before
            .iter()
            .cloned()
            .map(|entry| (entry.code, entry))
            .collect();
        let after: Vec<CharacterEntry> = req
            .order
            .iter()
            .filter_map(|code| entries_by_code.remove(code))
            .collect();
        ObjectChange::CharacterSetChanged(CharacterSetChange {
            character_set_id: req.character_set_id,
            before,
            after,
        })
    };
    apply_single(doc, change)
}

/// Applies `RecodeCharacterEntry`, returning a change set whose warnings list any
/// glyphs left dangling by the recode.
pub fn recode_character_entry(
    doc: &mut FontSpace,
    req: &RecodeCharacterEntry,
) -> Result<ChangeSet, FontSpaceError> {
    if req.from_code == req.to_code {
        return Ok(ChangeSet::default());
    }
    let (change, warnings) = {
        let character_set = doc
            .character_set(req.character_set_id)
            .ok_or(FontSpaceError::CharacterSetIdNotFound(req.character_set_id))?;
        if !character_set.contains_code(req.from_code) {
            return Err(FontSpaceError::EntryCodeNotFound {
                character_set: req.character_set_id,
                code: req.from_code,
            });
        }
        if character_set.contains_code(req.to_code) {
            return Err(FontSpaceError::DuplicateEntryCode {
                character_set: req.character_set_id,
                code: req.to_code,
            });
        }
        let before = character_set.entries.clone();
        let after: Vec<CharacterEntry> = before
            .iter()
            .map(|entry| {
                if entry.code == req.from_code {
                    CharacterEntry {
                        code: req.to_code,
                        label: entry.label.clone(),
                    }
                } else {
                    entry.clone()
                }
            })
            .collect();

        // Glyphs across every glyph set referencing this character set that still
        // carry the old code are now dangling — warn, but move nothing.
        let orphaned: Vec<OrphanedGlyph> = doc
            .glyph_sets
            .iter()
            .filter(|glyph_set| glyph_set.character_set_id == req.character_set_id)
            .flat_map(|glyph_set| {
                glyph_set.pages.iter().flat_map(move |page| {
                    page.glyphs
                        .iter()
                        .filter(|glyph| glyph.code == req.from_code)
                        .map(move |glyph| OrphanedGlyph {
                            glyph_set_id: glyph_set.id,
                            page_id: page.id,
                            code: glyph.code,
                        })
                })
            })
            .collect();

        let warnings = if orphaned.is_empty() {
            Vec::new()
        } else {
            vec![FontSpaceWarning::RecodeOrphanedGlyphs {
                character_set_id: req.character_set_id,
                from_code: req.from_code,
                to_code: req.to_code,
                orphaned,
            }]
        };
        (
            ObjectChange::CharacterSetChanged(CharacterSetChange {
                character_set_id: req.character_set_id,
                before,
                after,
            }),
            warnings,
        )
    };

    let change_set = ChangeSet {
        object_changes: vec![change],
        warnings,
    };
    apply_change_set(doc, &change_set)?;
    Ok(change_set)
}

/// Applies `RemoveCharacterEntry`: removes the entry and cascade-deletes referencing
/// glyphs across the whole document, atomically, returning them in a warning.
pub fn remove_character_entry(
    doc: &mut FontSpace,
    req: &RemoveCharacterEntry,
) -> Result<ChangeSet, FontSpaceError> {
    let (object_changes, warnings) = {
        let character_set = doc
            .character_set(req.character_set_id)
            .ok_or(FontSpaceError::CharacterSetIdNotFound(req.character_set_id))?;
        if !character_set.contains_code(req.code) {
            return Err(FontSpaceError::EntryCodeNotFound {
                character_set: req.character_set_id,
                code: req.code,
            });
        }
        let before = character_set.entries.clone();
        let after: Vec<CharacterEntry> = before
            .iter()
            .filter(|entry| entry.code != req.code)
            .cloned()
            .collect();

        // The entry removal, then a GlyphRemoved for every referencing glyph.
        let mut object_changes = vec![ObjectChange::CharacterSetChanged(CharacterSetChange {
            character_set_id: req.character_set_id,
            before,
            after,
        })];
        let mut removed = Vec::new();
        for glyph_set in doc
            .glyph_sets
            .iter()
            .filter(|glyph_set| glyph_set.character_set_id == req.character_set_id)
        {
            for page in &glyph_set.pages {
                if let Some(index) = page.glyphs.iter().position(|glyph| glyph.code == req.code) {
                    object_changes.push(ObjectChange::GlyphRemoved(GlyphPlacement {
                        glyph_set_id: glyph_set.id,
                        page_id: page.id,
                        index,
                        code: req.code,
                        bitmap: page.glyphs[index].bitmap.clone(),
                    }));
                    removed.push(CascadedGlyph {
                        glyph_set_id: glyph_set.id,
                        page_id: page.id,
                        code: req.code,
                    });
                }
            }
        }
        let warnings = if removed.is_empty() {
            Vec::new()
        } else {
            vec![FontSpaceWarning::RemoveCascade {
                character_set_id: req.character_set_id,
                code: req.code,
                removed,
            }]
        };
        (object_changes, warnings)
    };

    let change_set = ChangeSet {
        object_changes,
        warnings,
    };
    apply_change_set(doc, &change_set)?;
    Ok(change_set)
}

/// Wraps one change into a warning-free change set and applies it.
fn apply_single(doc: &mut FontSpace, change: ObjectChange) -> Result<ChangeSet, FontSpaceError> {
    let change_set = ChangeSet {
        object_changes: vec![change],
        warnings: Vec::new(),
    };
    apply_change_set(doc, &change_set)?;
    Ok(change_set)
}
