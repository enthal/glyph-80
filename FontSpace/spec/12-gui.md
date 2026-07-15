# 12. GUI Structure and UX

## 12.1 Tiled shell

The shell uses `egui_tiles`. Every major view is a tile: Glyph Editor, Character Set, Page Overview, Text Preview, Character Across Pages/Files, Document Browser, Inspector, Export Configuration, Export Preview, and (later) a raw/logical-memory view.

Ship an opinionated default layout with the glyph editor dominant:

```text
+----------------+---------------------------------+--------------+
| Documents      | Glyph Editor                    | Inspector    |
| and objects    |                                 |              |
+----------------+---------------------------------+--------------+
| Character Set / Page Overview / Preview tabs     |              |
+--------------------------------------------------+--------------+
```

Required behaviors: drag/dock tiles, tab related views, resize splits, maximize/restore a tile, "focus glyph editor" command, reset-to-default layout, persistent layout restoration (chapter 11). Named layout presets are later.

**Widget-ID discipline:** any view may render in multiple tiles at once. Salt global `egui` IDs with the tile/document id; inside a tile prefer `ui.id().with(..)`; give looped/auto-id widgets a unique `id_salt`. A duplicate widget ID is a critical bug (see [../CLAUDE.md](../CLAUDE.md)).

## 12.2 Document browser

Each open file shows a tree of Character Sets, Glyph Sets (with their pages), and Export Configurations — **not** every glyph as a node.

```text
example.fontspace.json
- Character Sets
  - ASCII 7-bit
  - Hardware Symbols
- Glyph Sets
  - Terminal 8x16
    - Regular
    - Bold
  - Icons 16x16
    - Main
- Export Configurations
  - AT28C64 Text ROM
```

All open files are shown, the active one marked; clicking a page in any file **switches to that file** (making it active) and selects the page. Supports create, rename, duplicate, copy, paste, delete, reorder where meaningful, drag between open files where safe, open-in-new-tile, and (later) reveal-all-views-referencing-an-object.

## 12.3 Glyph editor

A custom painted widget, **not** a matrix of `Button`s. It shows the pixel matrix, grid lines, page guides, optional coordinate labels, hover coordinate, optional byte/bit info, and the selected page/code/character/glyph set.

The matrix fills available space while keeping pixels square:

```text
cell_size = max(1, floor(min(available_width / glyph_width, available_height / glyph_height)))
```

The clamp to at least one pixel keeps the matrix non-degenerate (and hover mapping division safe) when a tile is smaller than the glyph. The matrix is centered in the available space.

Supports fit-to-panel, integer zoom, mouse-wheel zoom, panning when zoomed past the viewport, and grid levels off/subtle/strong. The size math, hover mapping, and stroke logic are **pure functions** tested outside the paint closure (chapter 15).

## 12.4 Pointer editing: first pixel determines the stroke

```text
pointer-down on an OFF pixel -> the whole stroke paints ON
pointer-down on an ON pixel  -> the whole stroke erases
```

The operation is fixed for the entire drag. Implementation: capture the pointer until release; interpolate between sampled cells so fast motion leaves no gaps; visit each cell at most once per stroke; accumulate the changed cells; commit **one** `SetPixels` command on release (chapter 7); show the tentative stroke live before commit if practical. One drag = one undo entry.

## 12.5 Keyboard defaults (configurable later)

```text
Arrow keys        move pixel cursor
Space             toggle pixel
Delete            clear selection
Cmd/Ctrl+C / V    copy / paste
Cmd/Ctrl+Z        undo
Cmd/Ctrl+Shift+Z  redo
Cmd/Ctrl+O        open
Cmd/Ctrl+S        save
Cmd/Ctrl+Shift+S  save as
Cmd/Ctrl+W        close
[ and ]           previous / next character (by charset order)
Shift+[ and ]     previous / next page
```

## 12.6 Guides

Guides render as colored/highlighted lines drawn **between pixels** — on the grid lines, over the top of the matrix's own grid rendering — not through pixel centers. A guide's integer `position` is a grid-line coordinate: a horizontal guide at `position = n ≥ 0` sits on the boundary above row `n` (so `0` is the top edge and `height` is the bottom edge), and likewise a vertical guide at `position = n ≥ 0` sits on the boundary left of column `n`. A **negative** `position` is reckoned from the opposite edge — `-k` resolves to `extent − k` (where `extent` is the glyph's height for a horizontal guide, width for a vertical one), so a horizontal `-2` sits two pixels above the bottom edge and stays edge-relative as the glyph is resized. A position beyond the glyph bounds still draws outside the matrix (no clamping). Each guide draws in **its own stable color**, derived from its `GuideId` so the color survives renders and reorders, and shown as a swatch beside it in the guides list. Each guide is labeled near an edge.

Users can add horizontal/vertical guides, rename, drag with integer snapping (snapping to grid lines), type an exact position, show/hide, lock/unlock, and duplicate or copy to selected pages (copying mints fresh `GuideId`s — chapter 3).

## 12.7 Character-set view

An ordered table showing ordinal, `code`, rendered character where printable, label, and control-character notation. Users create/duplicate a set, add a slot or a range, remove/reorder slots, edit codes and labels, and apply the ASCII preset. The view shows the impact of an edit before applying it — especially a **remove** (which cascade-deletes glyphs, chapter 4) or a **recode** (which may orphan glyphs).

## 12.8 Page overview

Every glyph in a selected page (absent codes shown blank, dangling flagged). Controls: thumbnail size, auto/fixed columns, label mode (none/character/hex/full), grid overlay, blank indicator, modified indicator, filter/range. Interactions: click selects, **hover shows the glyph's info** (hex code, character/control notation, label), double-click opens the editor, drag-select a range, copy/paste, and apply shift/clear/invert to a selection.

## 12.9 Character across pages and files

Shows one `code` across selected pages, glyph sets, and files, with synchronized zoom, optional guides, normalized cell size, source labels (file / glyph set / page), direct copy from one source to another, and overlay/difference visualization. Comparing across geometries is allowed visually; copying across geometries still requires an explicit `GlyphSizeConversion` (chapter 8).

## 12.10 Text preview

Editable sample text, glyph-set/page selection, fg/bg colors, inversion, integer zoom, character/line spacing, wrap width, optional grid overlay, and sample presets (ASCII coverage, upper/lower, digits, punctuation, programming text, terminal output). Optional multi-page mode renders the same text per page. Glyph cells render **flush** by default (no inter-cell spacing); an optional 1px divider overlay (the grid overlay, off by default) separates them. Like the page overview, **clicking a glyph selects it** and **hovering shows its info** (hex code, character/control notation, label).

## 12.11 Inspector

Selection-driven. **Glyph:** file, glyph set, page, `code`, label, geometry, on-pixel count, raw row bytes, common transforms. **Page:** name, description, index, guide list, glyph count, blank count. **Export config:** source binding, validation state, transforms, packing, address/data maps, image size, output format.

## 12.12 Menus

`File · Edit · View · Character Set · Glyph · Page · Export · Window · Help`. Core file actions: New, Open, Open Recent, Save, Save As, Close, Revert, Export, Recover Unsaved Work.

The menu bar is defined **once** as a command tree and rendered by two presenters — a native macOS `NSMenu` (all top-level menus, not just the app menu) and an in-window `egui` menu bar on Linux/Windows. See [18-platform-support.md](18-platform-support.md) §18.3–18.4 for the registry, the macOS install timing/lifetime caveats, and how accelerators stay in sync with §12.5.

**Launch with a document.** A path passed on the command line (`fontspace-gui file.fontspace.json`) opens that document for editing on launch, replacing the in-memory starter in place (so no stray Untitled document is left beside it); a failed read is reported and leaves the starter untouched (§16.2). With no argument the app opens on the starter.

**File behavior.** Each open document has a bound file path (`None` until first saved) and a per-document `dirty` flag (chapter 11). **Save** writes the active document canonically via atomic replacement ([16-performance-safety-limits.md](16-performance-safety-limits.md) §16.2), falling back to **Save As** when it has never been saved; **Save As** chooses a new path and rebinds it. The active document's name and unsaved-changes state show both in the OS window title and as an in-bar marker.

**Open** opens the chosen file as a **new** document, making it active and moving the previous active document to the background (chapter 11 §11.1) — it discards nothing, so it is **not** guarded by an unsaved-changes prompt. **Revert**, by contrast, reloads the *active* document from disk in place, discarding that document's edits, so it first confirms when the active document is dirty; a failed load leaves every open document untouched (§16.2). The `dirty` flag is conservative: it stays set after undoing back to the last-saved state, which at worst asks for an unneeded confirmation and never risks silent loss. The default accelerators are `Cmd/Ctrl+O` (Open), `Cmd/Ctrl+S` (Save), `Cmd/Ctrl+Shift+S` (Save As), and `Cmd/Ctrl+W` (Close), per §12.5. **Close** discards nothing on its own — it removes the active document from the workspace, promoting the most-recently-active remaining document — but because that document's unsaved edits would be lost it carries its own unsaved-changes guard (the same confirmation as Revert). It is a no-op while only one document is open, since the workspace always keeps at least one; the empty-`New` document that would let the last one close arrives with a following slice. `New`, `Open Recent`, and `Recover Unsaved Work` arrive with the following workspace and recovery slices (chapter 11).
