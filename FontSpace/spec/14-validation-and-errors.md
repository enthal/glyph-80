# 14. Validation and Error Reporting

Validation happens at three levels. Errors are structured Rust types; GUI, CLI, and MCP render them. Warnings are non-fatal and travel in `ChangeSet.warnings` or a load report.

## 14.1 Document validation

On load (after migration) and on demand:

- supported `format_version`;
- unique stable IDs within the document (glyph sets, pages, guides, export configs, export components);
- valid `character_set_id` references from glyph sets;
- unique entry `code` within each character set;
- nonzero glyph dimensions within limits (chapter 16);
- each glyph's bitmap size equals its glyph set's `glyph_size`;
- **referential integrity** — every glyph's `code` exists in the referenced character set; a violation is a **dangling-glyph warning** (tolerated, chapter 4), not a hard error;
- **uniqueness** — at most one glyph per `code` per page (a duplicate is a hard error);
- structurally valid guides;
- structurally valid export configs and components.

## 14.2 Operation validation (before any mutation)

- resolve all selectors; reject ambiguous names with a precise error;
- reject missing objects and out-of-range coordinates;
- reject incompatible geometry unless a conversion policy is explicit (chapter 8);
- reject destructive remaps without an explicit policy;
- for `RemoveCharacterEntry`, compute the cascade set up front so the operation is atomic and the warning can list affected glyphs before applying.

## 14.3 Export validation (strict 1:1, chapter 10)

- exactly one source glyph set; `transforms.is_empty()`;
- compatible selected pages;
- the address map partitions the required code/page/pixel dimensions with no gap and no overlap, and **no dimension appears in both the address and data maps**;
- data-map width equals the source glyph width (row-scan) or height (column-scan);
- every selected code is addressable;
- computed image size within the configured limit.

## 14.4 Error quality

Errors identify object context as specifically as possible.

```text
Terminal 8x16 / Regular / code 0x41 / row 7: expected 8 pixels, found 7
```

```text
Export config "AT28C64 Text ROM" cannot be rendered:
glyph width is 16 but the data map has 8 output bits.
Add a split or packing transform when that feature is available.
```

```text
Character set "ASCII 7-bit": removing entry 0x41 will delete 3 glyphs
(Terminal 8x16 / Regular, Terminal 8x16 / Bold, Icons 16x16 / Main). [warning]
```

An error type carries the structured context (ids, code, coordinates); the human string is rendered from it, never the reverse.
