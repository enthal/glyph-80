# 4. Character Sets

A character set is an ordered list of character slots. It is a reusable object: multiple glyph sets (of possibly different geometry) may reference the same character set.

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterSet {
    pub id: CharacterSetId,
    pub name: String,
    pub description: String,
    pub entries: Vec<CharacterEntry>,   // ordered
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterEntry {
    pub code: u32,       // identity + ROM address dimension; required and unique within the set
    pub label: String,   // human name (e.g. "NUL", "LATIN CAPITAL A", "hardware-heart")
}
```

## 4.1 The identity decision: `code`, not position, not UUID

An entry is identified by its **`code: u32`**, which is required and unique within the character set. This is the single most important modeling decision in FontSpace, so it is stated precisely.

Three distinct concepts must never be conflated:

- **`code`** — the entry's identity and its ROM address dimension. For a Unicode character, `code` equals the Unicode scalar value (`A` = `0x41`, `NUL` = `0x00`). For a machine-specific or decorative slot with no Unicode meaning, `code` is a designer-chosen number (use the Private Use Area, `0xE000`+, to avoid collision). Glyphs reference their entry by `code`.
- **ordinal** — the entry's position in `entries`. It drives canonical glyph storage order and human display order. It is **not** stored as a field (it is implicit in array order) and it is **not** what the ROM addresses.
- **`label`** — the human-readable name. Carries no identity and need not be unique.

Rationale:

- **Stable under the common edit.** Reordering entries changes ordinals but not codes, so glyph references never break. Adding an entry is a no-op for existing glyphs. This dissolves the cross-object cascade that a positional model would require.
- **Hardware-meaningful.** The Glyph-80 display hardware addresses the font ROM by character code (the byte in display RAM), not by list position. Making `code` the identity means the export model addresses exactly what the hardware does (chapter 10).
- **Diff-friendly.** A stored glyph reads `{ "code": "0x41", ... }` — self-describing, no UUID indirection.

Changing an entry's `code` is semantically "this slot is now a different character"; it may orphan glyphs that referenced the old code. That is tolerated and warned (§4.4), not silently repaired. Reordering, adding, and (with cascade) removing entries never orphan.

The model must not assume `ordinal == code`, even though 7-bit ASCII happens to align that way. A printable-only set (`0x20`–`0x7E`) has `ordinal 0` at `code 0x20`.

## 4.2 Ordering

`entries` is ordered, and that order is canonical for both display and the on-disk order of a page's glyphs (chapter 6). Keeping entries sorted ascending by `code` yields the most stable diffs and is the recommended default; the built-in ASCII preset is stored that way. Reordering is a deliberate, visible edit that reshuffles display order and stored glyph order together, but changes no glyph data.

## 4.3 The ASCII preset

The initial built-in preset is 7-bit ASCII: 128 ordered entries with `code` `0x00`–`0x7F` and standard labels (`NUL`, `SOH`, …, `A`, `B`, …, `~`, `DEL`). For ASCII, `ordinal == code`. The preset is applied by an operation (`ApplyAsciiPreset` / a "new character set from preset" command), not hard-coded into any type.

## 4.4 Invariants and cascades

The strict length invariant of a positional model is replaced by two local, cheaply-checked invariants plus explicit edit semantics.

**Invariants (validated per page in O(page)):**

1. **Referential integrity.** Every glyph's `code` exists as an entry `code` in the glyph set's referenced character set — except transiently, as a *dangling* glyph (below).
2. **Uniqueness.** At most one glyph per `code` per page.
3. **Entry-code uniqueness.** Within a character set, `code` values are unique.

**Edit semantics:**

- **Add entry** — no glyph changes anywhere; the new code renders blank until a glyph is drawn for it.
- **Reorder entries** — no glyph changes; only display and canonical storage order shift.
- **Edit an entry's `label`** — no glyph changes.
- **Edit an entry's `code`** — may orphan glyphs that referenced the old code. The operation reports the affected glyphs as warnings; it does not move or delete them. (A distinct "recode and migrate glyphs" operation may be offered later; v1 does the simple thing and warns.)
- **Remove entry** — **cascade-delete** every glyph with that `code` across **all pages of every glyph set** referencing this character set. The removals are captured in the `ChangeSet` (so undo restores them) and surfaced as a warning listing the affected glyphs (glyph set / page / code). See [07-operations.md](07-operations.md) and [14-validation-and-errors.md](14-validation-and-errors.md).

**Dangling glyphs on load.** A file may contain a glyph whose `code` has no matching entry (hand-edited files, external tools, an entry removed by a tool that did not cascade). The loader **tolerates** this: it loads the glyph, emits a warning identifying it, and the glyph is treated as blank/unaddressable for rendering and export until the user resolves it (draw the entry back, or delete the glyph). Loading never silently drops or silently repairs a dangling glyph.

## 4.5 What a page stores

Because pages are sparse (chapter 3 §3.6), a page stores a glyph only for the codes it actually defines with non-blank pixels. Blank glyphs are pruned on save (chapter 6). A fully-blank page stores an empty `glyphs` array regardless of how many entries the character set has.

## 4.6 Changing a referenced character set

Because glyphs reference entries by `code`, most character-set edits require no coordinated page mutation — this is the whole point of the design. The only edit that touches pages is **remove entry** (cascade-delete, above). Everything else (add, reorder, relabel) leaves pages untouched, and a `code` edit only warns. This is dramatically simpler and safer than a positional model, where every add/remove/reorder would have to permute glyph vectors across every referencing page in lockstep.
