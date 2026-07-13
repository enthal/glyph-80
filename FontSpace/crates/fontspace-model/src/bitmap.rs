//! The `Bitmap` type: packed binary glyph pixels with method-only access and the
//! padding-bit-zero invariant (spec/05).
//!
//! Pixel meaning is fixed: `false` = off, `true` = on. Display inversion is a
//! rendering concern and is never stored (spec/05 §5.1, invariant in spec/17).

use crate::{GlyphSize, GuideAxis};

/// Packed binary glyph pixels.
///
/// Rows are byte-aligned and packed MSB-first: bit `x` of a row lives in byte
/// `x / 8` at bit position `7 - (x % 8)`. The fields are private; **no caller may
/// depend on the packing layout** — all access goes through methods (spec/05 §5.2).
///
/// **Padding-bit-zero invariant:** when `width` is not a multiple of 8, the unused
/// low bits of each row's last byte are always zero. Every constructor and mutator
/// maintains this — structurally, because the only writers ([`set`](Bitmap::set),
/// [`toggle`](Bitmap::toggle)) bounds-check `x < width` and so can never touch a
/// padding column. `Eq` derives over `packed_rows`, so this invariant is what makes
/// two semantically-equal bitmaps compare byte-equal (spec/05 §5.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bitmap {
    width: u16,
    height: u16,
    packed_rows: Vec<u8>,
}

impl Bitmap {
    /// A bitmap of the given size with every pixel off.
    pub fn new_blank(size: GlyphSize) -> Self {
        let len = size.bytes_per_row() * size.height as usize;
        Self {
            width: size.width,
            height: size.height,
            packed_rows: vec![0u8; len],
        }
    }

    pub fn width(&self) -> u16 {
        self.width
    }

    pub fn height(&self) -> u16 {
        self.height
    }

    /// The geometry of this bitmap.
    pub fn size(&self) -> GlyphSize {
        GlyphSize::new(self.width, self.height)
    }

    fn bytes_per_row(&self) -> usize {
        self.size().bytes_per_row()
    }

    /// Builds a bitmap by evaluating `f(x, y)` for every pixel in row-major order.
    /// Only `true` results are written (via `set_bit`), so padding columns are never
    /// touched and the padding-bit-zero invariant holds by construction. The shared
    /// engine behind [`shifted`], [`flipped`], and [`inverted`].
    fn from_fn(size: GlyphSize, mut f: impl FnMut(u16, u16) -> bool) -> Self {
        let mut out = Self::new_blank(size);
        for y in 0..size.height {
            for x in 0..size.width {
                if f(x, y) {
                    out.set_bit(x, y, true);
                }
            }
        }
        out
    }

    /// Byte index and MSB-first bit position for a coordinate assumed in-bounds.
    fn locate(&self, x: u16, y: u16) -> (usize, u8) {
        let byte = y as usize * self.bytes_per_row() + (x as usize / 8);
        let bit = 7 - (x % 8) as u8;
        (byte, bit)
    }

    /// Reads a pixel assumed in-bounds (internal; callers guarantee validity).
    fn get_bit(&self, x: u16, y: u16) -> bool {
        let (byte, bit) = self.locate(x, y);
        (self.packed_rows[byte] >> bit) & 1 == 1
    }

    /// Writes a pixel assumed in-bounds (internal; callers guarantee validity).
    /// Only ever called with `x < width`, so it cannot disturb a padding bit.
    fn set_bit(&mut self, x: u16, y: u16, value: bool) {
        let (byte, bit) = self.locate(x, y);
        if value {
            self.packed_rows[byte] |= 1 << bit;
        } else {
            self.packed_rows[byte] &= !(1 << bit);
        }
    }

    fn check_bounds(&self, x: u16, y: u16) -> Result<(), BitmapError> {
        if x < self.width && y < self.height {
            Ok(())
        } else {
            Err(BitmapError::OutOfBounds {
                x,
                y,
                width: self.width,
                height: self.height,
            })
        }
    }

    /// Reads a pixel, or `Err` for a coordinate outside the bitmap.
    pub fn get(&self, x: u16, y: u16) -> Result<bool, BitmapError> {
        self.check_bounds(x, y)?;
        Ok(self.get_bit(x, y))
    }

    /// Writes a pixel, or `Err` for a coordinate outside the bitmap.
    pub fn set(&mut self, x: u16, y: u16, value: bool) -> Result<(), BitmapError> {
        self.check_bounds(x, y)?;
        self.set_bit(x, y, value);
        Ok(())
    }

    /// Flips a pixel, or `Err` for a coordinate outside the bitmap.
    pub fn toggle(&mut self, x: u16, y: u16) -> Result<(), BitmapError> {
        self.check_bounds(x, y)?;
        let value = !self.get_bit(x, y);
        self.set_bit(x, y, value);
        Ok(())
    }

    /// Turns every pixel off.
    pub fn clear(&mut self) {
        self.packed_rows.fill(0);
    }

    /// True when every pixel is off.
    pub fn is_blank(&self) -> bool {
        self.packed_rows.iter().all(|&byte| byte == 0)
    }

    /// The number of on pixels. Relies on the padding-bit-zero invariant: because
    /// padding bits are never set, a popcount over the packed bytes equals the
    /// count of on pixels exactly.
    pub fn count_on(&self) -> u32 {
        self.packed_rows.iter().map(|byte| byte.count_ones()).sum()
    }

    #[cfg(test)]
    fn packed(&self) -> &[u8] {
        &self.packed_rows
    }
}

/// Why a bitmap coordinate access failed (spec/05 §5.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BitmapError {
    #[error("pixel ({x}, {y}) is out of bounds for a {width}×{height} bitmap")]
    OutOfBounds {
        x: u16,
        y: u16,
        width: u16,
        height: u16,
    },
}

/// How pixels shifted past an edge are handled (spec/07 §7.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverflowPolicy {
    /// Pixels shifted off an edge are lost; vacated pixels become off.
    Discard,
    /// Pixels shifted off one edge reappear on the opposite edge (toroidal).
    Wrap,
}

/// A copy of `src` translated by `(dx, dy)` under `overflow` (spec/05 §5.7).
///
/// Positive `dx`/`dy` move content right/down. `shifted(b, 0, 0, _) == b`.
pub fn shifted(src: &Bitmap, dx: i16, dy: i16, overflow: OverflowPolicy) -> Bitmap {
    // `w`/`h` are only read inside the closure, which `from_fn` calls solely for
    // `x in 0..width`, `y in 0..height` — so both are > 0 whenever it runs, and the
    // `rem_euclid(w)`/`rem_euclid(h)` below can never divide by zero.
    let (w, h) = (src.width as i32, src.height as i32);
    Bitmap::from_fn(src.size(), |x, y| {
        // Destination (x, y) samples the source at (x - dx, y - dy).
        let sx = x as i32 - dx as i32;
        let sy = y as i32 - dy as i32;
        match overflow {
            OverflowPolicy::Discard => {
                (0..w).contains(&sx) && (0..h).contains(&sy) && src.get_bit(sx as u16, sy as u16)
            }
            OverflowPolicy::Wrap => src.get_bit(sx.rem_euclid(w) as u16, sy.rem_euclid(h) as u16),
        }
    })
}

/// A copy of `src` mirrored across `axis` (spec/05 §5.7).
/// `flipped(flipped(b, a), a) == b`.
pub fn flipped(src: &Bitmap, axis: GuideAxis) -> Bitmap {
    Bitmap::from_fn(src.size(), |x, y| {
        let (sx, sy) = match axis {
            GuideAxis::Horizontal => (x, src.height - 1 - y),
            GuideAxis::Vertical => (src.width - 1 - x, y),
        };
        src.get_bit(sx, sy)
    })
}

/// A copy of `src` with every pixel toggled — a data operation, distinct from
/// display inversion (spec/05 §5.7). `inverted(inverted(b)) == b`.
pub fn inverted(src: &Bitmap) -> Bitmap {
    Bitmap::from_fn(src.size(), |x, y| !src.get_bit(x, y))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn blank(w: u16, h: u16) -> Bitmap {
        Bitmap::new_blank(GlyphSize::new(w, h))
    }

    #[test]
    fn new_blank_is_blank_with_correct_packed_length() {
        let b = blank(8, 16);
        assert_eq!(b.width(), 8);
        assert_eq!(b.height(), 16);
        assert!(b.is_blank());
        assert_eq!(b.count_on(), 0);
        assert_eq!(b.packed().len(), 16); // 1 byte/row * 16 rows
    }

    #[test]
    fn odd_width_uses_ceil_bytes_per_row() {
        assert_eq!(blank(5, 3).packed().len(), 3); // ceil(5/8)=1 byte/row
        assert_eq!(blank(9, 2).packed().len(), 4); // ceil(9/8)=2 bytes/row
        assert_eq!(blank(12, 1).packed().len(), 2);
    }

    #[test]
    fn set_get_toggle_roundtrip() {
        let mut b = blank(8, 8);
        assert!(!b.get(3, 4).unwrap());
        b.set(3, 4, true).unwrap();
        assert!(b.get(3, 4).unwrap());
        assert_eq!(b.count_on(), 1);
        b.toggle(3, 4).unwrap();
        assert!(!b.get(3, 4).unwrap());
        b.toggle(3, 4).unwrap();
        assert!(b.get(3, 4).unwrap());
        b.set(3, 4, false).unwrap();
        assert!(b.is_blank());
    }

    #[test]
    fn out_of_bounds_access_errors() {
        let mut b = blank(5, 3);
        let oob = BitmapError::OutOfBounds {
            x: 5,
            y: 0,
            width: 5,
            height: 3,
        };
        assert_eq!(b.get(5, 0), Err(oob));
        assert_eq!(b.set(5, 0, true), Err(oob));
        assert_eq!(b.toggle(5, 0), Err(oob));
        assert_eq!(
            b.get(0, 3),
            Err(BitmapError::OutOfBounds {
                x: 0,
                y: 3,
                width: 5,
                height: 3
            })
        );
        // A rejected write leaves the bitmap untouched.
        assert!(b.is_blank());
    }

    #[test]
    fn clear_turns_everything_off() {
        let mut b = blank(8, 2);
        b.set(0, 0, true).unwrap();
        b.set(7, 1, true).unwrap();
        assert!(!b.is_blank());
        b.clear();
        assert!(b.is_blank());
        assert_eq!(b.count_on(), 0);
    }

    #[test]
    fn equality_and_clone_are_content_based() {
        let mut a = blank(8, 8);
        let mut b = blank(8, 8);
        a.set(2, 3, true).unwrap();
        b.set(2, 3, true).unwrap();
        assert_eq!(a, b);
        assert_eq!(a, a.clone());
        b.set(4, 5, true).unwrap();
        assert_ne!(a, b);
    }

    /// The padding-bit-zero invariant, asserted directly on packed bytes for widths
    /// not divisible by 8 (spec/05 §5.3). Setting the rightmost valid column must
    /// leave the low padding bits zero.
    #[test]
    fn padding_bits_stay_zero_for_odd_widths() {
        // width 5: valid columns are bits 7..=3; bits 2,1,0 are padding.
        let mut b = blank(5, 1);
        b.set(4, 0, true).unwrap(); // bit position 7 - 4 = 3
        assert_eq!(b.packed(), &[0b0000_1000]);

        for x in 0..5 {
            b.set(x, 0, true).unwrap();
        }
        assert_eq!(b.packed(), &[0b1111_1000]); // columns 0..5 on, padding still zero
        assert_eq!(b.count_on(), 5); // popcount matches pixel count => no padding leak

        // width 12 spans two bytes: second byte holds columns 8..12 in bits 7..=4,
        // bits 3,2,1,0 padding.
        let mut c = blank(12, 1);
        for x in 0..12 {
            c.set(x, 0, true).unwrap();
        }
        assert_eq!(c.packed(), &[0b1111_1111, 0b1111_0000]);
        assert_eq!(c.count_on(), 12);
    }

    #[test]
    fn full_odd_width_bitmap_counts_exactly() {
        // If any padding bit leaked, count_on would exceed width*height.
        for w in [1u16, 5, 7, 9, 12, 15] {
            let mut b = blank(w, 3);
            for y in 0..3 {
                for x in 0..w {
                    b.set(x, y, true).unwrap();
                }
            }
            assert_eq!(b.count_on(), w as u32 * 3, "width {w}");
        }
    }

    #[test]
    fn shifted_discard_moves_and_drops() {
        let mut b = blank(4, 1);
        b.set(0, 0, true).unwrap();
        b.set(1, 0, true).unwrap();
        let s = shifted(&b, 2, 0, OverflowPolicy::Discard);
        // columns 0,1 -> 2,3
        assert!(!s.get(0, 0).unwrap());
        assert!(!s.get(1, 0).unwrap());
        assert!(s.get(2, 0).unwrap());
        assert!(s.get(3, 0).unwrap());
        // shift further right: everything drops off.
        assert!(shifted(&b, 4, 0, OverflowPolicy::Discard).is_blank());
    }

    #[test]
    fn shifted_wrap_wraps_around() {
        let mut b = blank(4, 1);
        b.set(3, 0, true).unwrap();
        let s = shifted(&b, 1, 0, OverflowPolicy::Wrap);
        assert!(s.get(0, 0).unwrap()); // column 3 wrapped to 0
        assert_eq!(s.count_on(), 1);
    }

    #[test]
    fn shifted_discard_vertical_and_diagonal() {
        let mut b = blank(3, 3);
        b.set(0, 0, true).unwrap();
        // Down by 1.
        let down = shifted(&b, 0, 1, OverflowPolicy::Discard);
        assert!(down.get(0, 1).unwrap());
        assert!(!down.get(0, 0).unwrap());
        // Diagonal (right 1, down 1).
        let diag = shifted(&b, 1, 1, OverflowPolicy::Discard);
        assert!(diag.get(1, 1).unwrap());
        assert_eq!(diag.count_on(), 1);
        // Up by 1 pushes the only pixel off the top edge.
        assert!(shifted(&b, 0, -1, OverflowPolicy::Discard).is_blank());
    }

    #[test]
    fn shifted_wrap_vertical() {
        let mut b = blank(1, 3);
        b.set(0, 0, true).unwrap();
        // Up by 1 wraps the top pixel to the bottom row.
        let s = shifted(&b, 0, -1, OverflowPolicy::Wrap);
        assert!(s.get(0, 2).unwrap());
        assert_eq!(s.count_on(), 1);
    }

    #[test]
    fn flipped_horizontal_and_vertical() {
        let mut b = blank(2, 2);
        b.set(0, 0, true).unwrap(); // top-left
        let v = flipped(&b, GuideAxis::Vertical); // mirror left<->right
        assert!(v.get(1, 0).unwrap());
        let h = flipped(&b, GuideAxis::Horizontal); // mirror top<->bottom
        assert!(h.get(0, 1).unwrap());
    }

    #[test]
    fn flipped_asymmetric_full_content() {
        // 3×2 "L":  row0: X X .   row1: X . .
        let mut b = blank(3, 2);
        for (x, y) in [(0, 0), (1, 0), (0, 1)] {
            b.set(x, y, true).unwrap();
        }
        // Vertical mirror (x -> width-1-x):  row0: . X X   row1: . . X
        let v = flipped(&b, GuideAxis::Vertical);
        let on_v: Vec<(u16, u16)> = (0..2)
            .flat_map(|y| (0..3).map(move |x| (x, y)))
            .filter(|&(x, y)| v.get(x, y).unwrap())
            .collect();
        assert_eq!(on_v, vec![(1, 0), (2, 0), (2, 1)]);
        // Horizontal mirror (y -> height-1-y):  row0: X . .   row1: X X .
        let h = flipped(&b, GuideAxis::Horizontal);
        let on_h: Vec<(u16, u16)> = (0..2)
            .flat_map(|y| (0..3).map(move |x| (x, y)))
            .filter(|&(x, y)| h.get(x, y).unwrap())
            .collect();
        assert_eq!(on_h, vec![(0, 0), (0, 1), (1, 1)]);
    }

    // --- Property tests: the laws from spec/05 §5.7 ---
    //
    // The involution/identity laws below are all satisfied by a no-op, so they are
    // paired with reference-comparison proptests (`*_matches_reference`) that assert
    // exact per-pixel placement via the public `get` path — those are what actually
    // pin direction and catch an accidental identity or an x/y axis swap.

    prop_compose! {
        fn arb_bitmap()(w in 1u16..=17, h in 1u16..=17)
                       (bits in prop::collection::vec(any::<bool>(), w as usize * h as usize),
                        w in Just(w), h in Just(h))
                       -> Bitmap {
            let mut b = Bitmap::new_blank(GlyphSize::new(w, h));
            for y in 0..h {
                for x in 0..w {
                    if bits[y as usize * w as usize + x as usize] {
                        b.set(x, y, true).unwrap();
                    }
                }
            }
            b
        }
    }

    proptest! {
        #[test]
        fn shift_by_zero_is_identity(b in arb_bitmap(), wrap in any::<bool>()) {
            let policy = if wrap { OverflowPolicy::Wrap } else { OverflowPolicy::Discard };
            prop_assert_eq!(shifted(&b, 0, 0, policy), b);
        }

        #[test]
        fn flip_twice_is_identity(b in arb_bitmap(), vertical in any::<bool>()) {
            let axis = if vertical { GuideAxis::Vertical } else { GuideAxis::Horizontal };
            prop_assert_eq!(flipped(&flipped(&b, axis), axis), b);
        }

        #[test]
        fn invert_twice_is_identity(b in arb_bitmap()) {
            prop_assert_eq!(inverted(&inverted(&b)), b);
        }

        #[test]
        fn wrap_shift_preserves_pixel_count(b in arb_bitmap(), dx in -20i16..=20, dy in -20i16..=20) {
            prop_assert_eq!(shifted(&b, dx, dy, OverflowPolicy::Wrap).count_on(), b.count_on());
        }

        /// Asserts exact per-pixel placement for both policies and all shift
        /// directions — the reference re-derives each destination independently via
        /// `get`, so a no-op, a wrong sign, or an x/y `rem_euclid` swap fails here.
        #[test]
        fn shifted_matches_reference(
            b in arb_bitmap(),
            dx in -20i16..=20,
            dy in -20i16..=20,
            wrap in any::<bool>(),
        ) {
            let overflow = if wrap { OverflowPolicy::Wrap } else { OverflowPolicy::Discard };
            let out = shifted(&b, dx, dy, overflow);
            let (w, h) = (b.width() as i32, b.height() as i32);
            for y in 0..b.height() {
                for x in 0..b.width() {
                    let sx = x as i32 - dx as i32;
                    let sy = y as i32 - dy as i32;
                    let expected = match overflow {
                        OverflowPolicy::Discard => {
                            (0..w).contains(&sx)
                                && (0..h).contains(&sy)
                                && b.get(sx as u16, sy as u16).unwrap()
                        }
                        OverflowPolicy::Wrap => {
                            b.get(sx.rem_euclid(w) as u16, sy.rem_euclid(h) as u16).unwrap()
                        }
                    };
                    prop_assert_eq!(out.get(x, y).unwrap(), expected, "at ({}, {})", x, y);
                }
            }
        }

        /// Exact per-pixel placement for `flipped` on both axes.
        #[test]
        fn flipped_matches_reference(b in arb_bitmap(), vertical in any::<bool>()) {
            let axis = if vertical { GuideAxis::Vertical } else { GuideAxis::Horizontal };
            let out = flipped(&b, axis);
            for y in 0..b.height() {
                for x in 0..b.width() {
                    let (sx, sy) = match axis {
                        GuideAxis::Horizontal => (x, b.height() - 1 - y),
                        GuideAxis::Vertical => (b.width() - 1 - x, y),
                    };
                    prop_assert_eq!(out.get(x, y).unwrap(), b.get(sx, sy).unwrap());
                }
            }
        }

        /// `inverted` actually toggles every pixel (not just the involution law).
        #[test]
        fn inverted_toggles_every_pixel(b in arb_bitmap()) {
            let inv = inverted(&b);
            for y in 0..b.height() {
                for x in 0..b.width() {
                    prop_assert_eq!(inv.get(x, y).unwrap(), !b.get(x, y).unwrap());
                }
            }
            prop_assert_eq!(inv.count_on() + b.count_on(), b.width() as u32 * b.height() as u32);
        }
    }
}
