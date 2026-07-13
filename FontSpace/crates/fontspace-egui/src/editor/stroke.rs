//! Pure stroke logic for pixel editing (spec/12 §12.4): the first pixel fixes the
//! whole stroke to paint-on or erase, fast pointer motion is interpolated so no cell
//! is skipped, and each cell is visited at most once. The accumulated cells become
//! the edits of a single `SetPixels` command committed on release — one drag, one
//! undo entry (spec/07 §7.4, §7.7).
//!
//! All of this is pure and unit-tested here, outside the `egui` paint/input closure
//! (CLAUDE.md: "stroke logic … pure functions tested outside the paint closure").

use std::collections::HashSet;

use fontspace_ops::PixelEdit;

/// The integer cells on the line between two glyph cells (inclusive of both ends),
/// via Bresenham — used to fill gaps between sampled pointer positions so a fast
/// drag leaves no holes (spec/12 §12.4).
pub fn line_cells(from: (u16, u16), to: (u16, u16)) -> Vec<(u16, u16)> {
    let (mut x0, mut y0) = (from.0 as i32, from.1 as i32);
    let (x1, y1) = (to.0 as i32, to.1 as i32);
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    let mut cells = Vec::new();
    loop {
        // Endpoints come from resolved in-bounds glyph cells, so the cast is exact.
        cells.push((x0 as u16, y0 as u16));
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
    cells
}

/// An in-progress editor stroke: the fixed paint value (set by the first pixel) and
/// the ordered, de-duplicated cells visited so far.
#[derive(Debug, Clone)]
pub struct Stroke {
    /// `true` = the stroke paints pixels on; `false` = it erases. Fixed for the whole
    /// drag (spec/12 §12.4).
    pub paint_on: bool,
    cells: Vec<(u16, u16)>,
    seen: HashSet<(u16, u16)>,
    last: Option<(u16, u16)>,
}

impl Stroke {
    /// Begins a stroke painting `paint_on` (the caller passes `!current_pixel` so a
    /// stroke that starts on an off pixel paints on, and vice versa).
    pub fn begin(paint_on: bool) -> Self {
        Stroke {
            paint_on,
            cells: Vec::new(),
            seen: HashSet::new(),
            last: None,
        }
    }

    /// Extends the stroke to `cell`, interpolating from the previous sample so no
    /// cell between the two is skipped. Each cell is added at most once.
    pub fn extend_to(&mut self, cell: (u16, u16)) {
        let segment = match self.last {
            Some(prev) => line_cells(prev, cell),
            None => vec![cell],
        };
        for c in segment {
            if self.seen.insert(c) {
                self.cells.push(c);
            }
        }
        self.last = Some(cell);
    }

    /// The cells painted so far, in visit order (for the live tentative preview).
    pub fn cells(&self) -> &[(u16, u16)] {
        &self.cells
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// The pixel edits this stroke commits: every visited cell set to `paint_on`.
    pub fn edits(&self) -> Vec<PixelEdit> {
        self.cells
            .iter()
            .map(|&(x, y)| PixelEdit {
                x,
                y,
                value: self.paint_on,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_cells_single_point() {
        assert_eq!(line_cells((3, 4), (3, 4)), vec![(3, 4)]);
    }

    #[test]
    fn line_cells_horizontal_and_vertical() {
        assert_eq!(
            line_cells((0, 0), (3, 0)),
            vec![(0, 0), (1, 0), (2, 0), (3, 0)]
        );
        assert_eq!(
            line_cells((2, 5), (2, 2)),
            vec![(2, 5), (2, 4), (2, 3), (2, 2)]
        );
    }

    #[test]
    fn line_cells_diagonal_leaves_no_gap() {
        // A perfect diagonal steps one cell each axis per step; no holes.
        assert_eq!(
            line_cells((0, 0), (3, 3)),
            vec![(0, 0), (1, 1), (2, 2), (3, 3)]
        );
    }

    #[test]
    fn stroke_interpolates_between_samples() {
        // Two far-apart samples still fill the line between them.
        let mut stroke = Stroke::begin(true);
        stroke.extend_to((0, 0));
        stroke.extend_to((4, 0));
        assert_eq!(stroke.cells(), &[(0, 0), (1, 0), (2, 0), (3, 0), (4, 0)]);
    }

    #[test]
    fn stroke_visits_each_cell_once() {
        // Doubling back over already-painted cells does not repeat them.
        let mut stroke = Stroke::begin(true);
        stroke.extend_to((0, 0));
        stroke.extend_to((2, 0));
        stroke.extend_to((0, 0));
        assert_eq!(stroke.cells(), &[(0, 0), (1, 0), (2, 0)]);
    }

    #[test]
    fn stroke_edits_carry_the_fixed_paint_value() {
        let mut stroke = Stroke::begin(false); // an erase stroke
        stroke.extend_to((1, 1));
        stroke.extend_to((2, 1));
        let edits = stroke.edits();
        assert_eq!(edits.len(), 2);
        assert!(edits.iter().all(|e| !e.value));
        assert_eq!((edits[0].x, edits[0].y), (1, 1));
    }
}
