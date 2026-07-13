//! Pure argument parsing for the CLI (spec/13.1). Kept out of the command handlers
//! so it is unit-testable without any I/O (CLAUDE.md: logic testable without a UI
//! must not live inside a UI function).

use fontspace_model::{FontSpace, GlyphSetId, OverflowPolicy};
use fontspace_ops::{GlyphSelector, PageSelector};
use fontspace_render::RenderLayout;

/// Why a command-line value could not be parsed or resolved.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ParseError {
    #[error("invalid code {0:?} (expected a character, decimal, or 0x hex)")]
    Code(String),
    #[error("invalid overflow {0:?} (expected 'discard' or 'wrap')")]
    Overflow(String),
    #[error("invalid layout {0:?}")]
    Layout(String),
    #[error("invalid pixel {0:?} (expected X,Y,VALUE where VALUE is 0/1)")]
    Pixel(String),
    #[error("no glyph set named {0:?}")]
    GlyphSetNotFound(String),
    #[error("glyph set name {name:?} is ambiguous ({count} match)")]
    AmbiguousGlyphSet { name: String, count: usize },
}

/// A single pixel assignment parsed from `X,Y,VALUE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParsedPixel {
    pub x: u16,
    pub y: u16,
    pub value: bool,
}

/// Parses one code token: a single non-digit character is its Unicode scalar
/// (`A` → 0x41); `0x`/`0X` prefix is hex; otherwise decimal (spec/13.1).
pub fn parse_code_token(token: &str) -> Result<u32, ParseError> {
    let token = token.trim();
    let err = || ParseError::Code(token.to_string());
    let mut chars = token.chars();
    let first = chars.next().ok_or_else(err)?;
    if chars.next().is_none() && !first.is_ascii_digit() {
        // Exactly one character, and not a digit: treat as a Unicode scalar.
        return Ok(first as u32);
    }
    match token
        .strip_prefix("0x")
        .or_else(|| token.strip_prefix("0X"))
    {
        Some(hex) => u32::from_str_radix(hex, 16).map_err(|_| err()),
        None => token.parse::<u32>().map_err(|_| err()),
    }
}

/// Parses a glyph selector: `all`; a `START-END` range; a comma list of codes; or a
/// single code (spec/13.1). Ranges and codes accept characters, decimal, or `0x` hex.
pub fn parse_glyph_selector(spec: &str) -> Result<GlyphSelector, ParseError> {
    let spec = spec.trim();
    if spec.eq_ignore_ascii_case("all") {
        return Ok(GlyphSelector::All);
    }
    if spec.contains(',') {
        let codes = spec
            .split(',')
            .map(parse_code_token)
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(GlyphSelector::Codes(codes));
    }
    if let Some((start, end)) = spec.split_once('-') {
        return Ok(GlyphSelector::CodeRangeInclusive {
            start: parse_code_token(start)?,
            end: parse_code_token(end)?,
        });
    }
    Ok(GlyphSelector::Code(parse_code_token(spec)?))
}

/// Parses a page selector: `all`, or a comma list of page names (spec/13.1).
pub fn parse_page_selector(spec: &str) -> PageSelector {
    let spec = spec.trim();
    if spec.eq_ignore_ascii_case("all") {
        return PageSelector::All;
    }
    let names: Vec<String> = spec
        .split(',')
        .map(|name| name.trim().to_string())
        .collect();
    match names.as_slice() {
        [single] => PageSelector::Name(single.clone()),
        _ => PageSelector::Names(names),
    }
}

/// Parses an overflow policy: `discard` or `wrap`.
pub fn parse_overflow(value: &str) -> Result<OverflowPolicy, ParseError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "discard" => Ok(OverflowPolicy::Discard),
        "wrap" => Ok(OverflowPolicy::Wrap),
        _ => Err(ParseError::Overflow(value.to_string())),
    }
}

/// Parses a render layout: `glyphs-horizontal`, `glyphs-vertical`, `grid:N`,
/// `pages-horizontal`, `pages-vertical`.
pub fn parse_layout(value: &str) -> Result<RenderLayout, ParseError> {
    let value = value.trim().to_ascii_lowercase();
    if let Some(columns) = value.strip_prefix("grid:") {
        return columns
            .parse::<usize>()
            .map(|columns| RenderLayout::Grid { columns })
            .map_err(|_| ParseError::Layout(value.clone()));
    }
    match value.as_str() {
        "glyphs-horizontal" => Ok(RenderLayout::GlyphsHorizontal),
        "glyphs-vertical" => Ok(RenderLayout::GlyphsVertical),
        "pages-horizontal" => Ok(RenderLayout::PagesHorizontal),
        "pages-vertical" => Ok(RenderLayout::PagesVertical),
        _ => Err(ParseError::Layout(value)),
    }
}

/// Parses a `X,Y,VALUE` pixel edit; VALUE is `0`/`1` (or `on`/`off`).
pub fn parse_pixel(token: &str) -> Result<ParsedPixel, ParseError> {
    let err = || ParseError::Pixel(token.to_string());
    let mut parts = token.split(',');
    let x = parts
        .next()
        .ok_or_else(err)?
        .trim()
        .parse::<u16>()
        .map_err(|_| err())?;
    let y = parts
        .next()
        .ok_or_else(err)?
        .trim()
        .parse::<u16>()
        .map_err(|_| err())?;
    let value = match parts
        .next()
        .ok_or_else(err)?
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "1" | "on" | "true" => true,
        "0" | "off" | "false" => false,
        _ => return Err(err()),
    };
    if parts.next().is_some() {
        return Err(err());
    }
    Ok(ParsedPixel { x, y, value })
}

/// Resolves a glyph set by (unique) name to its id (spec/13.1 uses names on the CLI).
pub fn resolve_glyph_set(doc: &FontSpace, name: &str) -> Result<GlyphSetId, ParseError> {
    let mut matches = doc
        .glyph_sets
        .iter()
        .filter(|glyph_set| glyph_set.name == name);
    let first = matches
        .next()
        .ok_or_else(|| ParseError::GlyphSetNotFound(name.to_string()))?;
    let count = 1 + matches.count();
    if count > 1 {
        return Err(ParseError::AmbiguousGlyphSet {
            name: name.to_string(),
            count,
        });
    }
    Ok(first.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_tokens() {
        assert_eq!(parse_code_token("A").unwrap(), 0x41);
        assert_eq!(parse_code_token("0x41").unwrap(), 0x41);
        assert_eq!(parse_code_token("65").unwrap(), 65);
        assert_eq!(parse_code_token("5").unwrap(), 5); // single digit = decimal, not '5'
        assert_eq!(parse_code_token("0x7e").unwrap(), 0x7e);
        assert!(parse_code_token("").is_err());
        assert!(parse_code_token("0xZZ").is_err());
    }

    #[test]
    fn glyph_selectors() {
        assert_eq!(parse_glyph_selector("all").unwrap(), GlyphSelector::All);
        assert_eq!(
            parse_glyph_selector("A").unwrap(),
            GlyphSelector::Code(0x41)
        );
        assert_eq!(
            parse_glyph_selector("A-Z").unwrap(),
            GlyphSelector::CodeRangeInclusive {
                start: 0x41,
                end: 0x5A
            }
        );
        assert_eq!(
            parse_glyph_selector("0x20-0x7E").unwrap(),
            GlyphSelector::CodeRangeInclusive {
                start: 0x20,
                end: 0x7E
            }
        );
        assert_eq!(
            parse_glyph_selector("A,B,0x43").unwrap(),
            GlyphSelector::Codes(vec![0x41, 0x42, 0x43])
        );
    }

    #[test]
    fn page_selectors() {
        assert_eq!(parse_page_selector("all"), PageSelector::All);
        assert_eq!(
            parse_page_selector("Regular"),
            PageSelector::Name("Regular".into())
        );
        assert_eq!(
            parse_page_selector("Regular,Bold"),
            PageSelector::Names(vec!["Regular".into(), "Bold".into()])
        );
    }

    #[test]
    fn overflow_and_layout() {
        assert_eq!(parse_overflow("discard").unwrap(), OverflowPolicy::Discard);
        assert_eq!(parse_overflow("WRAP").unwrap(), OverflowPolicy::Wrap);
        assert!(parse_overflow("nope").is_err());
        assert_eq!(
            parse_layout("glyphs-horizontal").unwrap(),
            RenderLayout::GlyphsHorizontal
        );
        assert_eq!(
            parse_layout("grid:4").unwrap(),
            RenderLayout::Grid { columns: 4 }
        );
        assert!(parse_layout("grid:x").is_err());
        assert!(parse_layout("bogus").is_err());
    }

    #[test]
    fn pixels() {
        assert_eq!(
            parse_pixel("3,2,1").unwrap(),
            ParsedPixel {
                x: 3,
                y: 2,
                value: true
            }
        );
        assert_eq!(
            parse_pixel("0,0,off").unwrap(),
            ParsedPixel {
                x: 0,
                y: 0,
                value: false
            }
        );
        assert!(parse_pixel("3,2").is_err());
        assert!(parse_pixel("3,2,maybe").is_err());
        assert!(parse_pixel("3,2,1,4").is_err());
    }
}
