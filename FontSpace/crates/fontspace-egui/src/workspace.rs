//! The multi-document workspace model (spec/11 §11.1). The GUI holds several open
//! FontSpace documents at once, each with a **runtime-only** identity that is never
//! persisted in any domain reference. This is workspace state, not document data
//! (CLAUDE.md), so it lives here in `fontspace-egui`, never in the core crates.
//!
//! This slice introduces the model and an active-document facade over [`AppState`]
//! while the app still holds exactly one document; opening several at once and
//! switching between them arrive in a following slice.

use std::path::{Path, PathBuf};

use fontspace_model::{FontSpace, IdGen};
use uuid::Uuid;

use crate::state::Selection;

/// A runtime-only identity for an open document (spec/11 §11.1). Regenerated every
/// launch and **never** written to a `.fontspace.json`; persisted workspace bindings
/// use a stable key (path or recovery id) instead (§11.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DocumentId(pub Uuid);

impl DocumentId {
    /// Mints a fresh id from the injected generator (never an ambient UUID — spec/03).
    pub fn new(ids: &mut dyn IdGen) -> Self {
        Self(ids.next_uuid())
    }
}

/// One open document: its identity, the file it is bound to (`None` until first
/// saved), its content, whether it has unsaved edits, and the per-document selection
/// (spec/11 §11.1, §11.4 — selection is per-document workspace state).
pub struct OpenDocument {
    pub id: DocumentId,
    pub path: Option<PathBuf>,
    pub content: FontSpace,
    pub dirty: bool,
    pub selection: Selection,
}

impl OpenDocument {
    /// Wraps freshly built content as a never-saved, clean open document.
    pub fn new(id: DocumentId, content: FontSpace, selection: Selection) -> Self {
        Self {
            id,
            path: None,
            content,
            dirty: false,
            selection,
        }
    }

    /// The document's display name: its file name, or `"Untitled"` when never saved.
    pub fn display_name(&self) -> String {
        self.path
            .as_deref()
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fontspace_model::{GlyphSetId, PageId, SequentialIdGen};

    fn selection() -> Selection {
        let mut ids = SequentialIdGen::new();
        Selection {
            glyph_set_id: GlyphSetId::new(&mut ids),
            page_id: PageId::new(&mut ids),
            code: 0x41,
        }
    }

    fn empty_doc(ids: &mut dyn IdGen) -> FontSpace {
        FontSpace::new(ids, "Doc", "")
    }

    #[test]
    fn document_ids_are_distinct_and_from_the_generator() {
        // Runtime ids come from the injected generator (never ambient — spec/03), so
        // two mints differ and are reproducible under a sequential generator.
        let mut ids = SequentialIdGen::new();
        let a = DocumentId::new(&mut ids);
        let b = DocumentId::new(&mut ids);
        assert_ne!(a, b);
    }

    #[test]
    fn a_never_saved_document_is_untitled_and_clean() {
        let mut ids = SequentialIdGen::new();
        let doc = OpenDocument::new(DocumentId::new(&mut ids), empty_doc(&mut ids), selection());
        assert_eq!(doc.display_name(), "Untitled");
        assert!(doc.path.is_none());
        assert!(!doc.dirty);
    }

    #[test]
    fn a_bound_document_shows_its_file_name() {
        let mut ids = SequentialIdGen::new();
        let mut doc =
            OpenDocument::new(DocumentId::new(&mut ids), empty_doc(&mut ids), selection());
        doc.path = Some(PathBuf::from("/fonts/terminal.fontspace.json"));
        assert_eq!(doc.display_name(), "terminal.fontspace.json");
    }
}
