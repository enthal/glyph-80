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
    #[error("choose only one render subject: --glyphs, --text, or --text-nl")]
    RenderSubjectConflict,
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

/// What `render-text` should draw: either a set-selection of glyph codes (the
/// `--glyphs` selector, or *all* codes by default) or an ordered run of text lines
/// (`--text` / `--text-nl`). These are mutually exclusive (spec/13.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderSubject {
    Glyphs(GlyphSelector),
    Text(Vec<Vec<u32>>),
}

/// Maps an input string to a single line of character `code`s: each `char`'s Unicode
/// scalar (equal to the 8-bit code for Latin-1 input). Repeats are preserved;
/// filtering of codes with no character-set entry happens at render time.
pub fn text_to_rows(text: &str) -> Vec<Vec<u32>> {
    vec![text.chars().map(|c| c as u32).collect()]
}

/// Like [`text_to_rows`], but a newline (`U+000A`) starts a new line instead of
/// mapping to a code. `"AB\nC"` → two lines `[AB]`, `[C]`.
pub fn text_nl_to_rows(text: &str) -> Vec<Vec<u32>> {
    text.split('\n')
        .map(|line| line.chars().map(|c| c as u32).collect())
        .collect()
}

/// Resolves the single render subject from the three mutually-exclusive flags,
/// defaulting to *all glyphs* when none is given (spec/13.1). Errors if more than
/// one is supplied.
pub fn resolve_render_subject(
    glyphs: Option<&str>,
    text: Option<&str>,
    text_nl: Option<&str>,
) -> Result<RenderSubject, ParseError> {
    let count = glyphs.is_some() as u8 + text.is_some() as u8 + text_nl.is_some() as u8;
    if count > 1 {
        return Err(ParseError::RenderSubjectConflict);
    }
    match (glyphs, text, text_nl) {
        (_, Some(t), _) => Ok(RenderSubject::Text(text_to_rows(t))),
        (_, _, Some(t)) => Ok(RenderSubject::Text(text_nl_to_rows(t))),
        (Some(g), _, _) => Ok(RenderSubject::Glyphs(parse_glyph_selector(g)?)),
        (None, None, None) => Ok(RenderSubject::Glyphs(GlyphSelector::All)),
    }
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

    #[test]
    fn text_rows_map_chars_to_codes_and_keep_repeats() {
        assert_eq!(text_to_rows("AAB"), vec![vec![0x41, 0x41, 0x42]]);
        // A plain newline is an ordinary char here (code 0x0A), not a line break.
        assert_eq!(text_to_rows("A\nB"), vec![vec![0x41, 0x0A, 0x42]]);
    }

    #[test]
    fn text_nl_rows_break_on_newline() {
        assert_eq!(text_nl_to_rows("AB\nC"), vec![vec![0x41, 0x42], vec![0x43]]);
        // A trailing newline yields a trailing empty line.
        assert_eq!(text_nl_to_rows("A\n"), vec![vec![0x41], vec![]]);
    }

    #[test]
    fn render_subject_defaults_to_all_glyphs() {
        assert_eq!(
            resolve_render_subject(None, None, None).unwrap(),
            RenderSubject::Glyphs(GlyphSelector::All)
        );
    }

    #[test]
    fn render_subject_from_text_flags() {
        assert_eq!(
            resolve_render_subject(None, Some("Hi"), None).unwrap(),
            RenderSubject::Text(vec![vec![0x48, 0x69]])
        );
        assert_eq!(
            resolve_render_subject(None, None, Some("A\nB")).unwrap(),
            RenderSubject::Text(vec![vec![0x41], vec![0x42]])
        );
    }

    #[test]
    fn render_subject_rejects_multiple_subjects() {
        assert_eq!(
            resolve_render_subject(Some("A-Z"), Some("Hi"), None),
            Err(ParseError::RenderSubjectConflict)
        );
        assert_eq!(
            resolve_render_subject(None, Some("Hi"), Some("Yo")),
            Err(ParseError::RenderSubjectConflict)
        );
    }
}
