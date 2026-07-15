# 11. Multi-Document Workspace

## 11.1 Open documents

The application holds multiple open FontSpace documents at once. Each has a runtime-only identity.

```rust
pub struct DocumentId(pub Uuid);   // runtime-only; never persisted in domain references

pub struct OpenDocument {
    pub id: DocumentId,
    pub path: Option<PathBuf>,   // None for a never-saved document
    pub content: FontSpace,
    pub dirty: bool,
    pub selection: Selection,    // active glyph set/page/code — per-document (§11.4)
}
```

The active **selection** (glyph set / page / code) is per-document runtime state and rides on `OpenDocument` so switching documents restores each one's selection; it is persisted as part of the per-document workspace state (§11.4), never in the `.fontspace.json`. `DocumentId` is minted from the injected `IdGen` (never an ambient UUID — spec/03).

## 11.2 Cross-document references

Views may refer to objects in different open documents (e.g. a comparison tile showing the same code from several files, or an export-preview tile binding a config in one document to a glyph set in another).

```rust
pub struct WorkspaceGlyphRef {
    pub document_id: DocumentId,
    pub glyph_set_id: GlyphSetId,
    pub page_id: PageId,
    pub code: u32,
}
```

Such bindings belong to **workspace state**, not to any `.fontspace.json`. A portable cross-file reference mechanism is future work.

## 11.3 Persisting references across restart

`DocumentId` is regenerated each launch, so persisted workspace bindings cannot store it directly. Workspace state persists a **stable per-open-document key** and remaps it to a fresh `DocumentId` on load:

- For a saved document, the stable key is its canonical file path.
- For a never-saved document, the stable key is a recovery-snapshot id (§11.6). Such a document is restored from its snapshot; if the snapshot is gone, the binding is dropped with a surfaced note, never silently.

On restore, each `OpenDocumentState` is loaded, assigned a new `DocumentId`, and every persisted binding's stable key is resolved to that new id. Unresolvable bindings are reported, not silently discarded.

## 11.4 Workspace state

```rust
pub struct WorkspaceState {
    pub format_version: u32,
    pub open_documents: Vec<OpenDocumentState>,
    pub active_document: Option<StableDocKey>,
    pub tile_layout: SavedTileTree,
    pub views: Vec<SavedViewState>,
    pub window_state: WindowState,
}
```

Per-document state includes: stable key (path or snapshot id), selected glyph set, selected page, selected code, active export config, and scroll/zoom where appropriate.

## 11.5 Persistence scopes

Four distinct scopes, never conflated:

- **FontSpace document** — character sets, glyph sets, pages, glyphs, guides, export configs. Explicit-save (unless optional autosave is enabled).
- **Workspace / session** — open files, tile layout, active views, selections, zoom/scroll, cross-document bindings, config-to-source bindings. Auto-persisted.
- **Application preferences** — theme, grid style, default display colors, key bindings, default pointer behavior, recent paths.
- **Recovery / autosave** — crash-recovery snapshots and unsaved-document recovery metadata.

## 11.6 Save behavior

- Workspace and preferences are auto-persisted: every meaningful change updates in-memory state; disk writes are **debounced** (250–1000 ms) and use **atomic replacement** (`workspace.json.tmp` → flush → rename). Save immediately on file open/close, major layout change, focus loss, shutdown, and explicit workspace switch.
- **User FontSpace documents remain explicit-save** (chapter 1 non-goals) unless the user opts into autosave. Recovery snapshots are a separate, crash-safety mechanism — they never overwrite the user's file.
- The undo/redo stack (chapter 7) is workspace-level so a cross-document transaction unwinds both sides together; per-document `dirty` flags update from whichever documents a transaction touched. Each entry is tagged with the `DocumentId` it applies to, so undo/redo targets the originating document even after the active document changes; reverting or closing a document drops only *its* entries from the stack.
