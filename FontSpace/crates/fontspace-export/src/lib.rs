#![forbid(unsafe_code)]

//! Strict 1:1 ROM/programmer export (spec/10).
//!
//! The Glyph-80 font ROM is addressed **by character `code`** (spec/10 §10.2): an
//! [`AddressMap`] says what each address line means, a [`DataMap`] says what each data
//! bit emits, and [`generate_image`] evaluates every address into a dense logical
//! memory image. [`validate_export`] proves a config is a real 1:1 mapping — each glyph
//! pixel to exactly one ROM bit, no gaps or overlaps — and explains any incompatibility
//! (spec/10 §10.7, spec/14 §14.3). [`encode_raw_binary`] is the v1 file encoder
//! (spec/10 §10.9); [`row_scan_config`] builds the standard row-scan text-ROM config.

use std::fmt;

use fontspace_model::{
    AddressBitSource, AddressMap, CoordinateExpr, DataMap, ExportComponentId, ExportConfig,
    ExportSourceSpec, GlyphSet, IdGen, Limits, OutputBitSource, OutputFormatConfig, PageId,
};
use thiserror::Error;

/// Why an export config is not a renderable strict-1:1 ROM (spec/10 §10.7, §14.4). Each
/// carries enough context to explain the incompatibility to the user.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ExportError {
    /// The address/data maps do not form a strict 1:1 mapping (spec/10 §10.3).
    #[error("export config {config:?} is not strict 1:1: {reason}")]
    NotStrictOneToOne { config: String, reason: String },
    /// The data map's width does not match the source glyph geometry (spec/10 §10.7).
    #[error(
        "export config {config:?}: {data_bits} ROM data bits but the glyph is {extent} \
         pixels {axis} — a strict 1:1 {scan}-scan needs one data bit per {axis}"
    )]
    DataWidthMismatch {
        config: String,
        data_bits: usize,
        extent: u16,
        axis: &'static str,
        scan: &'static str,
    },
    /// A drawn glyph's `code` falls outside the addressable code range (spec/10 §10.2).
    #[error(
        "export config {config:?}: code {code:#06x} is not addressable with \
         {code_bits} code bits (max {max:#06x})"
    )]
    CodeNotAddressable {
        config: String,
        code: u32,
        code_bits: u32,
        max: u32,
    },
    /// The address is wider than the configured limit (spec/16), or than this crate
    /// evaluates (≤ 32 lines).
    #[error(
        "export config {config:?}: address is {address_bits} bits, over the limit of \
         {max} (spec/16)"
    )]
    AddressTooWide {
        config: String,
        address_bits: usize,
        max: u32,
    },
    /// The dense image would exceed the configured word/pixel limit (spec/16).
    #[error("export config {config:?}: image is {words} words, over the limit of {max} (spec/16)")]
    ImageTooLarge {
        config: String,
        words: u64,
        max: u64,
    },
    /// The output format is not implemented in v1 (only `RawBinary`).
    #[error(
        "export config {config:?}: output format {name:?} is not supported yet (v1 is raw binary)"
    )]
    UnsupportedOutputFormat { config: String, name: String },
}

/// Which glyph axis the address scans; the other axis comes out on the data bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanDirection {
    /// The row (`pixel_y`) is addressed; the columns of that row are the data bits.
    Row,
    /// The column (`pixel_x`) is addressed; the rows of that column are the data bits.
    Column,
}

/// A validated 1:1 export's shape (spec/10 §10.3 success summary).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportSummary {
    pub scan: ScanDirection,
    pub glyph_width: u16,
    pub glyph_height: u16,
    pub data_bits: usize,
    pub code_bits: u32,
    pub codes: u64,
    pub pages: usize,
    pub output_bytes: u64,
}

impl fmt::Display for ExportSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (pixels_per_line, lines) = match self.scan {
            ScanDirection::Row => (self.glyph_width, self.glyph_height),
            ScanDirection::Column => (self.glyph_height, self.glyph_width),
        };
        let line = match self.scan {
            ScanDirection::Row => "row",
            ScanDirection::Column => "column",
        };
        writeln!(f, "Valid 1:1 export")?;
        writeln!(f, "{pixels_per_line} glyph pixels per {line}")?;
        writeln!(f, "{} ROM data bits", self.data_bits)?;
        writeln!(f, "{lines} {line}s per glyph")?;
        writeln!(f, "{} codes ({} code bits)", self.codes, self.code_bits)?;
        writeln!(f, "{} pages", self.pages)?;
        write!(f, "{} output bytes", self.output_bytes)
    }
}

/// The logical coordinates one ROM address decodes to (spec/10 §10.6).
#[derive(Debug, Clone, Copy, Default)]
struct AddressedCoords {
    code: u32,
    page: u32,
    x: u32,
    y: u32,
}

/// Decodes `address` into logical coordinates using the address map (spec/10 §10.6).
/// Constant lines carry no coordinate; `Inverted` flips the bit before it accumulates.
/// Bit indices beyond 31 are ignored (a malformed config caught by [`validate_export`]).
fn decode_address(config: &ExportConfig, address: u64) -> AddressedCoords {
    let mut coords = AddressedCoords::default();
    for (line, source) in config.address_map.address_bits.iter().enumerate() {
        let raw = (address >> line) & 1 != 0;
        accumulate(source, raw, &mut coords);
    }
    coords
}

fn accumulate(source: &AddressBitSource, raw: bool, coords: &mut AddressedCoords) {
    let set = |field: &mut u32, n: u8| {
        if raw {
            *field |= 1u32.checked_shl(n as u32).unwrap_or(0);
        }
    };
    match source {
        AddressBitSource::Constant(_) => {}
        AddressBitSource::CodeBit(n) => set(&mut coords.code, *n),
        AddressBitSource::PageBit(n) => set(&mut coords.page, *n),
        AddressBitSource::PixelXBit(n) => set(&mut coords.x, *n),
        AddressBitSource::PixelYBit(n) => set(&mut coords.y, *n),
        AddressBitSource::Inverted(inner) => accumulate(inner, !raw, coords),
    }
}

/// The ROM word at `address` (spec/10 §10.6): decode the address, select the page and
/// the glyph for that code (blank if the page or glyph is absent), then evaluate each
/// data bit. Infallible — a miss reads off — so failure lives in [`validate_export`].
/// Bit `i` of the result is `data_map.output_bits[i]` (i.e. `Di`).
pub fn evaluate_output_word(source: &GlyphSet, config: &ExportConfig, address: u64) -> u64 {
    let coords = decode_address(config, address);
    let bitmap = config
        .source
        .pages
        .get(coords.page as usize)
        .and_then(|page_id| source.page_of_id(*page_id))
        .and_then(|page| page.glyph_of_code(coords.code))
        .map(|glyph| &glyph.bitmap);

    let mut word = 0u64;
    for (i, output) in config.data_map.output_bits.iter().enumerate() {
        if eval_output_bit(output, bitmap, &coords) {
            word |= 1u64.checked_shl(i as u32).unwrap_or(0);
        }
    }
    word
}

fn eval_output_bit(
    source: &OutputBitSource,
    bitmap: Option<&fontspace_model::Bitmap>,
    coords: &AddressedCoords,
) -> bool {
    match source {
        OutputBitSource::Constant(v) => *v,
        OutputBitSource::Pixel { x, y } => {
            let px = resolve_coord(x, coords);
            let py = resolve_coord(y, coords);
            match (u16::try_from(px), u16::try_from(py)) {
                (Ok(px), Ok(py)) => bitmap.is_some_and(|b| b.get(px, py).unwrap_or(false)),
                _ => false, // a negative coordinate is off the glyph
            }
        }
        OutputBitSource::Inverted(inner) => !eval_output_bit(inner, bitmap, coords),
    }
}

fn resolve_coord(expr: &CoordinateExpr, coords: &AddressedCoords) -> i64 {
    match expr {
        CoordinateExpr::Constant(v) => *v as i64,
        CoordinateExpr::AddressedX => coords.x as i64,
        CoordinateExpr::AddressedY => coords.y as i64,
        CoordinateExpr::AddressedXPlus(v) => coords.x as i64 + *v as i64,
        CoordinateExpr::AddressedYPlus(v) => coords.y as i64 + *v as i64,
    }
}

/// The dense logical memory image (spec/10 §10.6): one word per address over the full
/// `2^address_bits` range. Fails if the address width or the image exceeds `limits`.
pub fn generate_image(
    source: &GlyphSet,
    config: &ExportConfig,
    limits: &Limits,
) -> Result<Vec<u64>, ExportError> {
    let address_bits = config.address_map.address_bits.len();
    let name = || config.name.clone();
    if address_bits as u32 > limits.max_export_address_width || address_bits > 32 {
        return Err(ExportError::AddressTooWide {
            config: name(),
            address_bits,
            max: limits.max_export_address_width.min(32),
        });
    }
    let words = 1u64 << address_bits;
    if words > limits.max_output_image_pixels {
        return Err(ExportError::ImageTooLarge {
            config: name(),
            words,
            max: limits.max_output_image_pixels,
        });
    }
    let image = (0..words)
        .map(|address| evaluate_output_word(source, config, address))
        .collect();
    Ok(image)
}

/// Encodes a logical image as a dense raw binary file (spec/10 §10.9): each word emits
/// `ceil(data_bits / 8)` little-endian bytes.
pub fn encode_raw_binary(image: &[u64], data_bits: usize) -> Vec<u8> {
    let bytes_per_word = data_bits.div_ceil(8).max(1);
    let mut out = Vec::with_capacity(image.len() * bytes_per_word);
    for &word in image {
        out.extend_from_slice(&word.to_le_bytes()[..bytes_per_word.min(8)]);
    }
    out
}

/// Validates that `config` is a **strict 1:1** ROM export of `source` (spec/10 §10.7,
/// §14.3) and returns its shape. Checks: the output format is supported; every address
/// line is a plain dimension bit; exactly one pixel axis is addressed (the other is on
/// the data bits, no dimension in both maps); each dimension's bits partition it with no
/// gap or overlap; the data map is a column/row permutation of the right width; every
/// drawn code is addressable; and the image is within `limits`.
pub fn validate_export(
    source: &GlyphSet,
    config: &ExportConfig,
    limits: &Limits,
) -> Result<ExportSummary, ExportError> {
    let name = config.name.clone();
    let not_1to1 = |reason: String| ExportError::NotStrictOneToOne {
        config: name.clone(),
        reason,
    };

    if let OutputFormatConfig::Unsupported { name: fmt_name } = &config.output_format {
        return Err(ExportError::UnsupportedOutputFormat {
            config: name.clone(),
            name: fmt_name.clone(),
        });
    }

    // Bound the address width up front, before any `1 << bits`: every dimension's bit
    // count is ≤ the address width, so guarding it here keeps all shifts below safe.
    let address_bits = config.address_map.address_bits.len();
    if address_bits as u32 > limits.max_export_address_width || address_bits > 32 {
        return Err(ExportError::AddressTooWide {
            config: name.clone(),
            address_bits,
            max: limits.max_export_address_width.min(32),
        });
    }
    let words = 1u64 << address_bits;
    if words > limits.max_output_image_pixels {
        return Err(ExportError::ImageTooLarge {
            config: name.clone(),
            words,
            max: limits.max_output_image_pixels,
        });
    }

    // Classify every address line into a dimension; reject constant/inverted lines,
    // which cannot appear in a strict-1:1 address (they would duplicate or drop coords).
    let (mut code_ns, mut page_ns, mut x_ns, mut y_ns) = (vec![], vec![], vec![], vec![]);
    for (line, source_bit) in config.address_map.address_bits.iter().enumerate() {
        match source_bit {
            AddressBitSource::CodeBit(n) => code_ns.push(*n),
            AddressBitSource::PageBit(n) => page_ns.push(*n),
            AddressBitSource::PixelXBit(n) => x_ns.push(*n),
            AddressBitSource::PixelYBit(n) => y_ns.push(*n),
            AddressBitSource::Constant(_) | AddressBitSource::Inverted(_) => {
                return Err(not_1to1(format!(
                    "address line A{line} is a constant/inverted line; a strict 1:1 map \
                     needs every line to be a plain code/page/pixel bit"
                )));
            }
        }
    }

    // Exactly one pixel axis in the address; the other is enumerated by the data bits.
    let scan = match (x_ns.is_empty(), y_ns.is_empty()) {
        (true, false) => ScanDirection::Row,
        (false, true) => ScanDirection::Column,
        (true, true) => {
            return Err(not_1to1(
                "no pixel dimension is addressed; put the row (pixel_y) or column \
                 (pixel_x) in the address map"
                    .into(),
            ));
        }
        (false, false) => {
            return Err(not_1to1(
                "both pixel dimensions are addressed; a strict 1:1 map addresses one \
                 axis and emits the other on the data bits"
                    .into(),
            ));
        }
    };

    if config.source.pages.is_empty() {
        return Err(not_1to1("no source pages selected".into()));
    }
    let size = source.glyph_size;
    let code_bits = partition(&mut code_ns, "code", &not_1to1)?;
    let page_bits = partition(&mut page_ns, "page", &not_1to1)?;
    if 1u64 << page_bits < config.source.pages.len() as u64 {
        return Err(not_1to1(format!(
            "page has {page_bits} address bits ({} values) but {} pages are selected",
            1u64 << page_bits,
            config.source.pages.len()
        )));
    }
    let (pixel_ns, pixel_axis, addressed_extent, data_extent, data_coord_is_addressed) = match scan
    {
        ScanDirection::Row => (&mut y_ns, "pixel_y", size.height, size.width, DataAxis::Y),
        ScanDirection::Column => (&mut x_ns, "pixel_x", size.width, size.height, DataAxis::X),
    };
    let pixel_bits = partition(pixel_ns, pixel_axis, &not_1to1)?;
    if 1u64 << pixel_bits < addressed_extent as u64 {
        return Err(not_1to1(format!(
            "{pixel_axis} has {pixel_bits} address bits ({} values) but the glyph is \
             {addressed_extent} {pixel_axis}",
            1u64 << pixel_bits
        )));
    }

    // The data map must be exactly one bit per column (row-scan) / row (column-scan),
    // each a pixel at the addressed line, together a permutation of 0..extent.
    let data_bits = config.data_map.output_bits.len();
    if data_bits != data_extent as usize {
        return Err(ExportError::DataWidthMismatch {
            config: name.clone(),
            data_bits,
            extent: data_extent,
            axis: match scan {
                ScanDirection::Row => "wide",
                ScanDirection::Column => "tall",
            },
            scan: match scan {
                ScanDirection::Row => "row",
                ScanDirection::Column => "column",
            },
        });
    }
    let mut covered = vec![false; data_extent as usize];
    for (i, output) in config.data_map.output_bits.iter().enumerate() {
        let coord = data_pixel_coord(output, data_coord_is_addressed).ok_or_else(|| {
            not_1to1(format!(
                "data bit D{i} is not a plain glyph pixel at the addressed line; a strict \
                 1:1 map emits one glyph pixel per data bit"
            ))
        })?;
        match usize::try_from(coord) {
            Ok(c) if c < covered.len() && !covered[c] => covered[c] = true,
            _ => {
                return Err(not_1to1(format!(
                    "data bit D{i} maps to an out-of-range or duplicated pixel; the data \
                     bits must be a permutation of 0..{data_extent}"
                )));
            }
        }
    }

    // Every drawn code on a selected page must fit the code bits.
    let max_code = (1u64 << code_bits).saturating_sub(1);
    for page_id in &config.source.pages {
        if let Some(page) = source.page_of_id(*page_id) {
            for glyph in &page.glyphs {
                if glyph.code as u64 > max_code {
                    return Err(ExportError::CodeNotAddressable {
                        config: name.clone(),
                        code: glyph.code,
                        code_bits,
                        max: max_code as u32,
                    });
                }
            }
        }
    }

    let output_bytes = words * data_bits.div_ceil(8).max(1) as u64;

    Ok(ExportSummary {
        scan,
        glyph_width: size.width,
        glyph_height: size.height,
        data_bits,
        code_bits,
        codes: 1u64 << code_bits,
        pages: config.source.pages.len(),
        output_bytes,
    })
}

/// Which addressed axis a row/column data pixel reads.
#[derive(Clone, Copy)]
enum DataAxis {
    X,
    Y,
}

/// For a strict-1:1 data bit — a `Pixel` whose *addressed* coordinate is the scanned
/// line and whose *other* coordinate is a constant column/row — returns that constant.
/// `None` for any other shape (constant bit, inverted bit, wrong coordinate kinds).
fn data_pixel_coord(output: &OutputBitSource, addressed: DataAxis) -> Option<i32> {
    let OutputBitSource::Pixel { x, y } = output else {
        return None;
    };
    match addressed {
        // Row-scan: y reads the addressed row, x is the constant column.
        DataAxis::Y => match (x, y) {
            (CoordinateExpr::Constant(cx), CoordinateExpr::AddressedY) => Some(*cx),
            _ => None,
        },
        // Column-scan: x reads the addressed column, y is the constant row.
        DataAxis::X => match (x, y) {
            (CoordinateExpr::AddressedX, CoordinateExpr::Constant(cy)) => Some(*cy),
            _ => None,
        },
    }
}

/// Checks that `ns` (a dimension's address-bit indices) partition `0..len` with no gap
/// or duplicate, returning the bit count. Sorts `ns` in place.
fn partition(
    ns: &mut [u8],
    dim: &str,
    not_1to1: &impl Fn(String) -> ExportError,
) -> Result<u32, ExportError> {
    ns.sort_unstable();
    for (expected, &n) in ns.iter().enumerate() {
        if n as usize != expected {
            return Err(not_1to1(format!(
                "the {dim} address bits must be a contiguous 0..N with no gap or \
                 duplicate; found {ns:?}"
            )));
        }
    }
    Ok(ns.len() as u32)
}

/// Builds the standard **row-scan** text-ROM config (spec/10 §10.4–10.5): the row
/// (`pixel_y`) and enough `code`/`page` bits go in the address (low→high: row, code,
/// page), and data bit `Di` emits pixel `x = width-1-i` of the addressed row — so the
/// leftmost pixel is the most-significant data bit. `code_bits` sets the addressable
/// code range; page bits are sized to `pages`.
pub fn row_scan_config(
    ids: &mut dyn IdGen,
    name: impl Into<String>,
    glyph_set: &GlyphSet,
    pages: Vec<PageId>,
    code_bits: u8,
) -> ExportConfig {
    let size = glyph_set.glyph_size;
    let y_bits = bits_for(size.height as u32);
    let page_bits = bits_for(pages.len() as u32);

    let mut address_bits = Vec::new();
    for n in 0..y_bits {
        address_bits.push(AddressBitSource::PixelYBit(n as u8));
    }
    for n in 0..code_bits {
        address_bits.push(AddressBitSource::CodeBit(n));
    }
    for n in 0..page_bits {
        address_bits.push(AddressBitSource::PageBit(n as u8));
    }

    let output_bits = (0..size.width)
        .map(|i| OutputBitSource::Pixel {
            // Di emits column (width-1-i): the leftmost pixel is the MSB (D_{width-1}).
            x: CoordinateExpr::Constant((size.width - 1 - i) as i32),
            y: CoordinateExpr::AddressedY,
        })
        .collect();

    ExportConfig {
        id: fontspace_model::ExportConfigId::new(ids),
        name: name.into(),
        description: String::new(),
        source: ExportSourceSpec {
            glyph_set_id: glyph_set.id,
            pages,
        },
        address_map: AddressMap {
            id: ExportComponentId::new(ids),
            name: "row-scan address".into(),
            address_bits,
        },
        data_map: DataMap {
            id: ExportComponentId::new(ids),
            name: "row-scan data".into(),
            output_bits,
        },
        output_format: OutputFormatConfig::RawBinary,
    }
}

/// Builds the standard **column-scan** ROM config: the column (`pixel_x`) and enough
/// `code`/`page` bits go in the address (low→high: column, code, page), and data bit
/// `Di` emits row `y = height-1-i` of the addressed column — so the top pixel is the
/// most-significant data bit. The counterpart to [`row_scan_config`] for hardware that
/// scans columns; each word is one glyph column.
pub fn column_scan_config(
    ids: &mut dyn IdGen,
    name: impl Into<String>,
    glyph_set: &GlyphSet,
    pages: Vec<PageId>,
    code_bits: u8,
) -> ExportConfig {
    let size = glyph_set.glyph_size;
    let x_bits = bits_for(size.width as u32);
    let page_bits = bits_for(pages.len() as u32);

    let mut address_bits = Vec::new();
    for n in 0..x_bits {
        address_bits.push(AddressBitSource::PixelXBit(n as u8));
    }
    for n in 0..code_bits {
        address_bits.push(AddressBitSource::CodeBit(n));
    }
    for n in 0..page_bits {
        address_bits.push(AddressBitSource::PageBit(n as u8));
    }

    let output_bits = (0..size.height)
        .map(|i| OutputBitSource::Pixel {
            x: CoordinateExpr::AddressedX,
            // Di emits row (height-1-i): the top pixel is the MSB (D_{height-1}).
            y: CoordinateExpr::Constant((size.height - 1 - i) as i32),
        })
        .collect();

    ExportConfig {
        id: fontspace_model::ExportConfigId::new(ids),
        name: name.into(),
        description: String::new(),
        source: ExportSourceSpec {
            glyph_set_id: glyph_set.id,
            pages,
        },
        address_map: AddressMap {
            id: ExportComponentId::new(ids),
            name: "column-scan address".into(),
            address_bits,
        },
        data_map: DataMap {
            id: ExportComponentId::new(ids),
            name: "column-scan data".into(),
            output_bits,
        },
        output_format: OutputFormatConfig::RawBinary,
    }
}

/// The number of address bits needed to index `count` values (`0` for 0/1).
fn bits_for(count: u32) -> u32 {
    if count <= 1 {
        0
    } else {
        u32::BITS - (count - 1).leading_zeros()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fontspace_model::{
        Bitmap, CharacterSetId, Glyph, GlyphPage, GlyphSet, GlyphSize, SequentialIdGen,
    };

    /// A glyph set of `size` with `page_count` empty pages, returning the ids of the
    /// pages so tests can draw on and address specific ones.
    fn glyph_set(
        ids: &mut SequentialIdGen,
        size: GlyphSize,
        page_count: usize,
    ) -> (GlyphSet, Vec<PageId>) {
        let charset = CharacterSetId::new(ids);
        let mut set = GlyphSet::new(ids, "Terminal", "", size, charset);
        let mut page_ids = Vec::new();
        for i in 0..page_count {
            let page = GlyphPage::new(ids, format!("page {i}"), "");
            page_ids.push(page.id);
            set.pages.push(page);
        }
        (set, page_ids)
    }

    fn draw(set: &mut GlyphSet, page: usize, code: u32, on: &[(u16, u16)]) {
        let size = set.glyph_size;
        let mut bitmap = Bitmap::new_blank(size);
        for &(x, y) in on {
            bitmap.set(x, y, true).unwrap();
        }
        set.pages[page].glyphs.push(Glyph { code, bitmap });
    }

    /// The MSB-first byte of glyph row `y` (x=0 is bit 7) — the value a row-scan ROM
    /// word should carry, computed independently of the evaluator.
    fn row_byte(bitmap: &Bitmap, y: u16) -> u8 {
        let mut b = 0u8;
        for x in 0..bitmap.width() {
            if bitmap.get(x, y).unwrap() {
                b |= 1 << (bitmap.width() - 1 - x);
            }
        }
        b
    }

    #[test]
    fn row_scan_emits_glyph_row_bytes_msb_first() {
        let mut ids = SequentialIdGen::new();
        let (mut set, pages) = glyph_set(&mut ids, GlyphSize::new(8, 8), 1);
        // Code 0 drawn; a lone pixel at (0,0) must land in the most-significant bit.
        draw(&mut set, 0, 0, &[(0, 0), (7, 0), (3, 2)]);
        let config = row_scan_config(&mut ids, "rom", &set, pages, 1);

        let summary = validate_export(&set, &config, &Limits::default()).unwrap();
        assert_eq!(summary.output_bytes, 16); // 4 address bits (3 row, 1 code) × 1 byte
        let image = generate_image(&set, &config, &Limits::default()).unwrap();
        let bytes = encode_raw_binary(&image, summary.data_bits);
        assert_eq!(bytes.len(), 16);

        // x=0 is the MSB, x=7 the LSB: (0,0)+(7,0) => 0b1000_0001.
        assert_eq!(bytes[0], 0b1000_0001);
        // Every code-0 row byte matches the independent computation; code 1 is blank.
        let glyph = &set.pages[0].glyph_of_code(0).unwrap().bitmap;
        for y in 0..8u16 {
            assert_eq!(bytes[y as usize], row_byte(glyph, y), "row {y}");
        }
        for byte in &bytes[8..16] {
            assert_eq!(*byte, 0, "code 1 is undrawn → blank");
        }
    }

    #[test]
    fn evaluate_selects_the_addressed_code_and_page() {
        let mut ids = SequentialIdGen::new();
        let (mut set, pages) = glyph_set(&mut ids, GlyphSize::new(8, 8), 2);
        // A single pixel on page index 1, code 3, row 0.
        draw(&mut set, 1, 3, &[(0, 0)]);
        let config = row_scan_config(&mut ids, "rom", &set, pages, 2);
        let image = generate_image(&set, &config, &Limits::default()).unwrap();

        // Address layout (low→high): 3 row bits, 2 code bits, 1 page bit.
        // row 0, code 3 (bits 3-4), page 1 (bit 5).
        let address = (3 << 3) | (1 << 5);
        assert_eq!(image[address], 0x80, "only (0,0) of page1/code3 is on");
        assert_eq!(
            image.iter().filter(|&&w| w != 0).count(),
            1,
            "exactly one nonzero word"
        );
    }

    #[test]
    fn validate_reports_the_at28c64_style_shape() {
        let mut ids = SequentialIdGen::new();
        let (set, pages) = glyph_set(&mut ids, GlyphSize::new(8, 16), 4);
        let config = row_scan_config(&mut ids, "AT28C64 Text ROM", &set, pages, 7);
        let summary = validate_export(&set, &config, &Limits::default()).unwrap();
        assert_eq!(summary.scan, ScanDirection::Row);
        assert_eq!(summary.data_bits, 8);
        assert_eq!(summary.code_bits, 7);
        assert_eq!(summary.codes, 128);
        assert_eq!(summary.pages, 4);
        assert_eq!(summary.output_bytes, 8192); // 4+7+2 = 13 address bits
        let text = summary.to_string();
        assert!(text.contains("8 ROM data bits"), "{text}");
        assert!(text.contains("128 codes (7 code bits)"), "{text}");
        assert!(text.contains("8192 output bytes"), "{text}");
    }

    #[test]
    fn validate_rejects_a_data_width_mismatch() {
        let mut ids = SequentialIdGen::new();
        let (set, pages) = glyph_set(&mut ids, GlyphSize::new(16, 16), 1);
        // A row-scan config for a 16-wide glyph needs 16 data bits; the preset gives 16,
        // so drop one output bit to force the mismatch.
        let mut config = row_scan_config(&mut ids, "rom", &set, pages, 4);
        config.data_map.output_bits.pop();
        let err = validate_export(&set, &config, &Limits::default()).unwrap_err();
        assert!(
            matches!(
                err,
                ExportError::DataWidthMismatch {
                    data_bits: 15,
                    extent: 16,
                    ..
                }
            ),
            "{err}"
        );
    }

    #[test]
    fn validate_rejects_a_constant_address_line() {
        let mut ids = SequentialIdGen::new();
        let (set, pages) = glyph_set(&mut ids, GlyphSize::new(8, 8), 1);
        let mut config = row_scan_config(&mut ids, "rom", &set, pages, 3);
        config
            .address_map
            .address_bits
            .push(AddressBitSource::Constant(true));
        let err = validate_export(&set, &config, &Limits::default()).unwrap_err();
        assert!(
            matches!(err, ExportError::NotStrictOneToOne { .. }),
            "{err}"
        );
    }

    #[test]
    fn validate_rejects_an_unaddressable_code() {
        let mut ids = SequentialIdGen::new();
        let (mut set, pages) = glyph_set(&mut ids, GlyphSize::new(8, 8), 1);
        // Only 1 code bit (codes 0..1), but a glyph is drawn at code 5.
        draw(&mut set, 0, 5, &[(0, 0)]);
        let config = row_scan_config(&mut ids, "rom", &set, pages, 1);
        let err = validate_export(&set, &config, &Limits::default()).unwrap_err();
        assert!(
            matches!(
                err,
                ExportError::CodeNotAddressable {
                    code: 5,
                    code_bits: 1,
                    ..
                }
            ),
            "{err}"
        );
    }

    #[test]
    fn encode_raw_binary_packs_little_endian_words() {
        // 8-bit words: one byte each.
        assert_eq!(
            encode_raw_binary(&[0x00, 0xA5, 0xFF], 8),
            vec![0x00, 0xA5, 0xFF]
        );
        // 12-bit words: two little-endian bytes each.
        assert_eq!(
            encode_raw_binary(&[0x123, 0x0FF], 12),
            vec![0x23, 0x01, 0xFF, 0x00]
        );
    }

    #[test]
    fn column_scan_emits_glyph_column_bytes_on_a_non_square_glyph() {
        let mut ids = SequentialIdGen::new();
        // 8 wide × 16 tall: width ≠ height catches an axis swap.
        let (mut set, pages) = glyph_set(&mut ids, GlyphSize::new(8, 16), 1);
        // A lone top pixel (0,0) must land in the MSB of column 0's 16-bit word.
        draw(&mut set, 0, 0, &[(0, 0), (0, 15)]);
        let config = column_scan_config(&mut ids, "col", &set, pages, 1);

        let summary = validate_export(&set, &config, &Limits::default()).unwrap();
        assert_eq!(summary.scan, ScanDirection::Column);
        assert_eq!(summary.data_bits, 16); // one data bit per row
        // address bits: 3 column (width 8) + 1 code = 4 → 16 words × 2 bytes = 32 bytes.
        assert_eq!(summary.output_bytes, 32);

        let image = generate_image(&set, &config, &Limits::default()).unwrap();
        // Column 0 of code 0: top pixel (0,0) → bit 15; bottom (0,15) → bit 0.
        assert_eq!(image[0], 0x8001);
        // Every other addressed column is blank (only column 0 drawn).
        assert_eq!(image.iter().filter(|&&w| w != 0).count(), 1);
    }

    #[test]
    fn validate_rejects_an_over_wide_address_without_panicking() {
        // A dimension with ≥64 bits would overflow a `1 << bits` shift; the address
        // guard must reject it up front with a typed error, never panic.
        let mut ids = SequentialIdGen::new();
        let (set, pages) = glyph_set(&mut ids, GlyphSize::new(8, 8), 1);
        let mut config = row_scan_config(&mut ids, "rom", &set, pages, 1);
        config.address_map.address_bits = (0..64).map(AddressBitSource::CodeBit).collect();
        let err = validate_export(&set, &config, &Limits::default()).unwrap_err();
        assert!(matches!(err, ExportError::AddressTooWide { .. }), "{err}");
    }

    #[test]
    fn evaluate_honors_inversion_and_coordinate_offsets() {
        use fontspace_model::{AddressMap, DataMap, ExportComponentId, ExportConfigId};
        let mut ids = SequentialIdGen::new();
        let (mut set, pages) = glyph_set(&mut ids, GlyphSize::new(4, 4), 1);
        draw(&mut set, 0, 0, &[(1, 0)]); // a single pixel at (1,0)
        let page_id = pages[0];

        // A hand-built config: address = inverted code bit 0 (so raw 0 → code 0) and a
        // single row bit; data D0 reads the pixel one column right of the addressed x
        // (AddressedXPlus(1)) at row 0, and D1 is its inversion.
        let config = ExportConfig {
            id: ExportConfigId::new(&mut ids),
            name: "hand".into(),
            description: String::new(),
            source: ExportSourceSpec {
                glyph_set_id: set.id,
                pages: vec![page_id],
            },
            address_map: AddressMap {
                id: ExportComponentId::new(&mut ids),
                name: "a".into(),
                // A0 = pixel_x bit 0 (addressed column), A1 = inverted code bit 0.
                address_bits: vec![
                    AddressBitSource::PixelXBit(0),
                    AddressBitSource::Inverted(Box::new(AddressBitSource::CodeBit(0))),
                ],
            },
            data_map: DataMap {
                id: ExportComponentId::new(&mut ids),
                name: "d".into(),
                output_bits: vec![
                    OutputBitSource::Pixel {
                        x: CoordinateExpr::AddressedXPlus(1),
                        y: CoordinateExpr::Constant(0),
                    },
                    OutputBitSource::Inverted(Box::new(OutputBitSource::Pixel {
                        x: CoordinateExpr::AddressedXPlus(1),
                        y: CoordinateExpr::Constant(0),
                    })),
                ],
            },
            output_format: OutputFormatConfig::RawBinary,
        };

        // Address 0: pixel_x=0; A1 raw 0, inverted → code bit set → code 1 (undrawn), so
        // D0 reads (1,0) off → 0, D1 = inversion → 1. Word = 0b10.
        assert_eq!(evaluate_output_word(&set, &config, 0), 0b10);
        // Address 2 (A1 raw 1, inverted → code 0): D0 reads (0+1, 0) = (1,0) = on → bit0,
        // D1 = inversion → 0. Word = 0b01.
        assert_eq!(evaluate_output_word(&set, &config, 0b10), 0b01);
    }

    #[test]
    fn bits_for_counts_the_address_bits() {
        assert_eq!(bits_for(1), 0);
        assert_eq!(bits_for(2), 1);
        assert_eq!(bits_for(4), 2);
        assert_eq!(bits_for(8), 3);
        assert_eq!(bits_for(16), 4);
        assert_eq!(bits_for(128), 7);
        assert_eq!(bits_for(129), 8); // not a power of two → rounds up
    }
}
