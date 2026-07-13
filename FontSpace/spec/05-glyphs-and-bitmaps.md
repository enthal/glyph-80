# 5. Glyphs and Bitmaps

## 5.1 Pixel semantics

A glyph pixel is binary and its meaning is fixed:

```text
false = off
true  = on
```

Display inversion (white-on-black vs black-on-white) is a rendering concern (chapter 9) and is **never** stored in glyph data.

## 5.2 The `Bitmap` type

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bitmap {
    width: u16,
    height: u16,
    packed_rows: Vec<u8>,   // row-major, each row byte-aligned; see packing
}

impl Bitmap {
    pub fn new_blank(size: GlyphSize) -> Self;
    pub fn width(&self) -> u16;
    pub fn height(&self) -> u16;
    pub fn get(&self, x: u16, y: u16) -> Result<bool, BitmapError>;
    pub fn set(&mut self, x: u16, y: u16, value: bool) -> Result<(), BitmapError>;
    pub fn toggle(&mut self, x: u16, y: u16) -> Result<(), BitmapError>;
    pub fn clear(&mut self);
    pub fn is_blank(&self) -> bool;      // all pixels off
    pub fn count_on(&self) -> u32;
}
```

No caller may depend on the packing layout; all access goes through methods. The fields are private.

## 5.3 Packing and the padding-bit invariant

Rows are byte-aligned: each row occupies `ceil(width / 8)` bytes, packed MSB-first (bit `x` of a row lives in byte `x / 8`, bit position `7 - (x % 8)`). The whole bitmap is `height * ceil(width / 8)` bytes.

**Padding-bit-zero invariant.** When `width` is not a multiple of 8, the low bits of each row's last byte are padding and **must always be zero**. Every mutating method (`set`, `toggle`, `clear`, construction) maintains this. The invariant is load-bearing: `PartialEq`/`Eq` derive over `packed_rows`, so two semantically-equal bitmaps must have identical bytes — a stray padding bit would make equal bitmaps compare unequal and corrupt round-trip and undo comparisons. This is a strict-layer test area (widths 5, 7, 9, 12, …).

## 5.4 Bounds and errors

`get`/`set`/`toggle` return `Err(BitmapError::OutOfBounds { x, y, width, height })` for coordinates outside the bitmap. Operations validate coordinates before mutating (chapter 7); the methods are the last line of defense, not the first.

## 5.5 Geometry agreement

A glyph's bitmap size must equal the owning glyph set's `glyph_size`. This is a document-validation check (chapter 14). Operations that place or paste a bitmap of a different size require an explicit `GlyphSizeConversion` policy (chapter 8) — never a silent resize.

## 5.6 Blank and sparse semantics

- A **blank** bitmap has every pixel off (`is_blank()` is true).
- Pages are **sparse**: a page holds a `Glyph` only for codes with non-blank content. An absent code renders blank.
- **On save, blank glyphs are pruned** (chapter 6): a `Glyph` whose bitmap `is_blank()` is not written. On load, absent codes are materialized lazily as blank when needed.
- **In memory, a page may hold an explicit blank glyph** during editing (e.g. the user clicked a blank slot to start drawing, then erased). This keeps every stroke a mutate-in-place `GlyphChanged` and keeps undo/redo of "draw then erase" a clean in-place pair, rather than an insert/remove dance. Sparsity is therefore a *storage* property, not an *editing-model* property. Canonicalization (prune-on-save) reconciles the two.

## 5.7 Pure operations

Bitmap transforms used by operations and export live here as pure functions and are unit-tested directly:

```rust
pub fn shifted(src: &Bitmap, dx: i16, dy: i16, overflow: OverflowPolicy) -> Bitmap;
pub fn flipped(src: &Bitmap, axis: GuideAxis) -> Bitmap;
pub fn inverted(src: &Bitmap) -> Bitmap;   // toggles every pixel (a data op, distinct from display inversion)
```

Properties (chapter 15): `shifted(b, 0, 0, _) == b`; `flipped(flipped(b, a), a) == b`; `inverted(inverted(b)) == b`.
