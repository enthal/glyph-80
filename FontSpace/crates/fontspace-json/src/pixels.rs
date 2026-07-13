//! Pure codecs for the two textual encodings in the format: a character `code` as a
//! hex string (spec/06 §6.3) and a glyph bitmap as visual rows (spec/06 §6.4).
//!
//! These are context-free and unit-tested directly; the caller ([`crate`]) wraps
//! any failure with object context.

use fontspace_model::{Bitmap, GlyphSize};

/// Canonical off/on pixel characters. Only these two are accepted in canonical files.
pub(crate) const OFF: char = '.';
pub(crate) const ON: char = '#';

/// Formats a code as a lowercase `0x` hex string, at least two digits (`0x41`,
/// `0x00`, `0xe000`). Always the canonical form written on save.
pub(crate) fn format_code(code: u32) -> String {
    format!("0x{code:02x}")
}

/// Parses a `code` leniently: a `0x`/`0X` prefix means hex, otherwise decimal
/// (spec/06 §6.3). Returns `None` for anything unparseable.
pub(crate) fn parse_code(text: &str) -> Option<u32> {
    let text = text.trim();
    match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(hex) => u32::from_str_radix(hex, 16).ok(),
        None => text.parse::<u32>().ok(),
    }
}

/// Formats a bitmap as visual rows: one string per row, `#` for on and `.` for off.
pub(crate) fn format_rows(bitmap: &Bitmap) -> Vec<String> {
    (0..bitmap.height())
        .map(|y| {
            (0..bitmap.width())
                .map(|x| {
                    if bitmap.get(x, y).unwrap_or(false) {
                        ON
                    } else {
                        OFF
                    }
                })
                .collect()
        })
        .collect()
}

/// Why a visual-row block did not describe a `size` bitmap. Context-free; the caller
/// attaches glyph set / page / code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RowError {
    RowCount {
        expected: u16,
        found: usize,
    },
    RowWidth {
        row: usize,
        expected: u16,
        found: usize,
    },
    InvalidChar {
        row: usize,
        column: usize,
        ch: char,
    },
}

/// Parses visual rows into a `size` bitmap, enforcing the spec/06 §6.4 loader rules:
/// exactly `height` rows, each exactly `width` Unicode scalars, only `.` and `#`.
pub(crate) fn parse_rows(rows: &[String], size: GlyphSize) -> Result<Bitmap, RowError> {
    if rows.len() != size.height as usize {
        return Err(RowError::RowCount {
            expected: size.height,
            found: rows.len(),
        });
    }
    let mut bitmap = Bitmap::new_blank(size);
    for (y, row) in rows.iter().enumerate() {
        let columns = row.chars().count();
        if columns != size.width as usize {
            return Err(RowError::RowWidth {
                row: y,
                expected: size.width,
                found: columns,
            });
        }
        for (x, ch) in row.chars().enumerate() {
            match ch {
                OFF => {}
                ON => {
                    // In-bounds by construction: y < height, x < width checked above.
                    let _ = bitmap.set(x as u16, y as u16, true);
                }
                other => {
                    return Err(RowError::InvalidChar {
                        row: y,
                        column: x,
                        ch: other,
                    });
                }
            }
        }
    }
    Ok(bitmap)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_code_is_lowercase_min_two_digits() {
        assert_eq!(format_code(0x41), "0x41");
        assert_eq!(format_code(0x00), "0x00");
        assert_eq!(format_code(0x7f), "0x7f");
        assert_eq!(format_code(0xE000), "0xe000");
    }

    #[test]
    fn parse_code_accepts_hex_and_decimal() {
        assert_eq!(parse_code("0x41"), Some(0x41));
        assert_eq!(parse_code("0X41"), Some(0x41));
        assert_eq!(parse_code("65"), Some(65)); // decimal
        assert_eq!(parse_code("  0x7f  "), Some(0x7f));
        assert_eq!(parse_code("0xe000"), Some(0xE000));
        assert_eq!(parse_code(""), None);
        assert_eq!(parse_code("0xZZ"), None);
        assert_eq!(parse_code("nope"), None);
    }

    #[test]
    fn code_round_trips_through_canonical_form() {
        for code in [0u32, 0x20, 0x41, 0x7f, 0xE000, 0x10_FFFF] {
            assert_eq!(parse_code(&format_code(code)), Some(code));
        }
    }

    #[test]
    fn rows_round_trip() {
        let size = GlyphSize::new(5, 3);
        let rows = vec![
            "#....".to_string(),
            ".#...".to_string(),
            "....#".to_string(),
        ];
        let bitmap = parse_rows(&rows, size).unwrap();
        assert!(bitmap.get(0, 0).unwrap());
        assert!(bitmap.get(1, 1).unwrap());
        assert!(bitmap.get(4, 2).unwrap());
        assert_eq!(bitmap.count_on(), 3);
        assert_eq!(format_rows(&bitmap), rows);
    }

    #[test]
    fn parse_rows_rejects_wrong_row_count() {
        let size = GlyphSize::new(2, 2);
        let err = parse_rows(&["##".to_string()], size).unwrap_err();
        assert_eq!(
            err,
            RowError::RowCount {
                expected: 2,
                found: 1
            }
        );
    }

    #[test]
    fn parse_rows_rejects_wrong_width_by_scalar_count() {
        let size = GlyphSize::new(3, 1);
        let err = parse_rows(&["##".to_string()], size).unwrap_err();
        assert_eq!(
            err,
            RowError::RowWidth {
                row: 0,
                expected: 3,
                found: 2
            }
        );
    }

    #[test]
    fn parse_rows_rejects_non_canonical_chars() {
        let size = GlyphSize::new(3, 1);
        let err = parse_rows(&[".x.".to_string()], size).unwrap_err();
        assert_eq!(
            err,
            RowError::InvalidChar {
                row: 0,
                column: 1,
                ch: 'x'
            }
        );
    }
}
