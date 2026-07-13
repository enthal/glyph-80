# 17. Invariants and Glossary

The load-bearing rules, consolidated. A regression against one of these is a P0 bug. Each links to its home chapter.

## 17.1 Invariants

- **Pixel meaning is fixed.** `false` = off, `true` = on; display inversion is never stored ([05](05-glyphs-and-bitmaps.md)).
- **Padding bits are zero.** For widths not divisible by 8, each row's unused low bits are always zero; every mutator maintains this, and `Eq` depends on it ([05](05-glyphs-and-bitmaps.md)).
- **Geometry agreement.** A glyph's bitmap size equals its glyph set's `glyph_size`; cross-size placement needs an explicit conversion ([05](05-glyphs-and-bitmaps.md), [08](08-fragments-and-clipboard.md)).
- **Entry identity is `code`.** Required and unique within a character set; it is the glyph reference key and the ROM address dimension; it is distinct from ordinal and label ([04](04-character-sets.md)).
- **Referential integrity + uniqueness replace the length invariant.** Every glyph's `code` exists in the referenced set (else it is a tolerated *dangling* glyph), and at most one glyph per `code` per page ([04](04-character-sets.md)).
- **Sparse pages; missing = blank.** A page stores a glyph only for defined codes; absent codes render blank; blank glyphs are pruned on save; canonical glyph order is charset-entry order ([04](04-character-sets.md), [05](05-glyphs-and-bitmaps.md), [06](06-json-persistence.md)).
- **Character-set edits don't cascade — except remove.** Reorder/add/relabel touch no glyphs; recode only warns; remove cascade-deletes referencing glyphs across all pages, atomically and invertibly, with a warning ([04](04-character-sets.md), [07](07-operations.md)).
- **Determinism.** No ambient UUID/clock/random in core code; canonical JSON is byte-stable; `save(load(save(d))) == save(d)` ([03](03-domain-model.md), [06](06-json-persistence.md)).
- **Atomicity.** Every batch/cross-document operation applies fully or not at all and returns one change set ([07](07-operations.md)).
- **One drag = one undo entry;** the undo stack is workspace-level ([07](07-operations.md), [11](11-workspace.md), [12](12-gui.md)).
- **No silent heuristics.** Ambiguous mappings and size conversions require an explicit policy; default is `RequireExact` ([08](08-fragments-and-clipboard.md)).
- **UI state is not document data.** Selections, layout, zoom, and cross-document bindings live in workspace state ([11](11-workspace.md)).
- **The core never depends on the GUI.** `model`/`ops`/`render`/`export`/`json` do not touch `egui` or platform APIs ([02](02-architecture.md)).
- **ROM is addressed by `code`.** The glyph dimension is the character code, not the ordinal; undefined codes emit blank words ([10](10-rom-export.md)).
- **User files are never corrupted.** Saves are atomic; failed load/migration never overwrites the source ([16](16-performance-safety-limits.md)).

## 17.2 Glossary

- **FontSpace document** — one `.fontspace.json` file; a container of character sets, glyph sets, and export configs.
- **Character set** — an ordered list of entries, reusable across glyph sets.
- **Entry** — a character slot: `{ code, label }`. Identified by `code`.
- **`code`** — a `u32`: the entry's identity and ROM address dimension; the Unicode scalar for Unicode characters, a designer-chosen value (PUA recommended) otherwise.
- **Ordinal** — an entry's position in its character set; drives display and canonical storage order, not ROM addressing.
- **Label** — an entry's human name; not an identity.
- **Glyph set** — one geometry + one character-set reference + pages.
- **Page** — a sparse set of glyphs within a glyph set; absent codes render blank.
- **Glyph** — `{ code, bitmap }`; the on/off pixels rendered for a code on a page.
- **Bitmap** — packed binary pixels with method-only access and the padding-bit-zero invariant.
- **Guide** — a named signed integer coordinate on a page (baseline, cap height, …).
- **Dangling glyph** — a glyph whose `code` has no entry in the referenced set; tolerated with a warning.
- **Blank glyph** — a bitmap with every pixel off; pruned on save.
- **Fragment** — a serializable slice of domain objects used for copy/paste.
- **Export config** — a persistent, named specification for producing a ROM/programmer image from a glyph set.
- **Address map / data map** — the per-bit definitions of ROM address lines and output bits.
- **Logical memory image** — the addressed array of output words, prior to file encoding.
- **Change set** — the invertible record of one operation; the undo/redo unit.
- **Workspace state** — session/UI state (open files, layout, selections, bindings); separate from documents.
- **`DocumentId`** — a runtime-only identity for an open document; never persisted in domain references.
- **`IdGen`** — the injected UUID source; `RandomIdGen` in production, `SequentialIdGen` in tests.
