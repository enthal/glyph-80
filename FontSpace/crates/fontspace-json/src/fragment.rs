//! Canonical JSON for copy/paste **fragments** (spec/08 §8.2, §8.5) — the
//! `application/x-fontspace-fragment+json` representation shared by the clipboard
//! and the CLI. It reuses the document format's textual codecs (hex `code`s, `.`/`#`
//! visual rows — spec/06 §6.3–6.4) and the same determinism contract:
//! `save_fragment(load_fragment(save_fragment(f))) == save_fragment(f)`, byte-for-byte.
//!
//! Like the document schema (spec/06 §6.2), the wire format is explicit `Stored*`
//! structs whose field order **is** the on-disk order. A fragment is tagged by a
//! `fragment_version` (for migration) and a `kind` (dispatching the variant); only
//! the `"glyphs"` kind exists so far (spec/08 §8.1).

use fontspace_model::{Bitmap, FontSpaceFragment, FragmentGlyph, GlyphFragment, GlyphSize, Limits};
use serde::{Deserialize, Serialize};

use crate::JsonError;
use crate::pixels::{format_code, format_rows, parse_rows};
use crate::storage::StoredGlyphSize;
use crate::{parse_code_in, row_error_to_json};

/// The fragment schema version this build reads and writes (spec/08 §8.5). Bumped
/// only by a breaking wire-format change, independently of the document
/// `format_version`.
pub const CURRENT_FRAGMENT_VERSION: u32 = 1;

const KIND_GLYPHS: &str = "glyphs";

/// A version/kind peek used to dispatch before full deserialization — the fragment
/// analogue of the document's `VersionPeek`.
#[derive(Deserialize)]
struct FragmentPeek {
    fragment_version: u32,
    kind: String,
}

/// The persisted glyph-fragment schema. Field declaration order is the canonical
/// on-disk order (spec/08 §8.5).
#[derive(Serialize, Deserialize)]
struct StoredGlyphFragment {
    fragment_version: u32,
    kind: String,
    source_glyph_size: StoredGlyphSize,
    #[serde(default)]
    glyphs: Vec<StoredFragmentGlyph>,
}

#[derive(Serialize, Deserialize)]
struct StoredFragmentGlyph {
    code: String,
    label: String,
    pixels: Vec<String>,
}

/// Serializes a fragment to canonical JSON (spec/08 §8.5). Infallible: the `Stored*`
/// schema is plain data and serializes without error, exactly like [`crate::save`].
pub fn save_fragment(fragment: &FontSpaceFragment) -> String {
    let mut json = match fragment {
        FontSpaceFragment::Glyphs(glyphs) => {
            serde_json::to_string_pretty(&to_stored_glyphs(glyphs))
        }
    }
    .expect("Stored* fragment schema (strings, numbers, vecs) serializes infallibly");
    json.push('\n');
    json
}

/// Parses canonical fragment JSON into a domain [`FontSpaceFragment`], enforcing the
/// version/kind tags, a valid `source_glyph_size`, and that every glyph's `pixels`
/// match that geometry (so the [`GlyphFragment`] per-glyph size invariant holds by
/// construction — spec/08 §8.1).
pub fn load_fragment(text: &str) -> Result<FontSpaceFragment, JsonError> {
    let peek: FragmentPeek = serde_json::from_str(text)?;
    if peek.fragment_version != CURRENT_FRAGMENT_VERSION {
        return Err(JsonError::UnsupportedFragmentVersion {
            found: peek.fragment_version,
            supported: CURRENT_FRAGMENT_VERSION,
        });
    }
    match peek.kind.as_str() {
        KIND_GLYPHS => {
            let stored: StoredGlyphFragment = serde_json::from_str(text)?;
            Ok(FontSpaceFragment::Glyphs(from_stored_glyphs(stored)?))
        }
        _ => Err(JsonError::UnknownFragmentKind { kind: peek.kind }),
    }
}

fn to_stored_glyphs(fragment: &GlyphFragment) -> StoredGlyphFragment {
    StoredGlyphFragment {
        fragment_version: CURRENT_FRAGMENT_VERSION,
        kind: KIND_GLYPHS.to_string(),
        source_glyph_size: StoredGlyphSize {
            width: fragment.source_glyph_size.width,
            height: fragment.source_glyph_size.height,
        },
        glyphs: fragment
            .glyphs
            .iter()
            .map(|glyph| StoredFragmentGlyph {
                code: format_code(glyph.code),
                label: glyph.label.clone(),
                pixels: format_rows(&glyph.bitmap),
            })
            .collect(),
    }
}

fn from_stored_glyphs(stored: StoredGlyphFragment) -> Result<GlyphFragment, JsonError> {
    let StoredGlyphFragment {
        fragment_version: _,
        kind: _,
        source_glyph_size,
        glyphs,
    } = stored;
    let source_glyph_size = GlyphSize::new(source_glyph_size.width, source_glyph_size.height);
    // A blank/degenerate geometry would make row parsing meaningless, so validate it
    // up front just as document load validates each glyph set's size (spec/14).
    source_glyph_size
        .validate(&Limits::default())
        .map_err(|err| JsonError::InvalidFragmentGeometry {
            width: source_glyph_size.width,
            height: source_glyph_size.height,
            reason: err.to_string(),
        })?;

    let glyphs = glyphs
        .into_iter()
        .map(|glyph| from_stored_glyph(glyph, source_glyph_size))
        .collect::<Result<Vec<_>, JsonError>>()?;
    Ok(GlyphFragment {
        source_glyph_size,
        glyphs,
    })
}

fn from_stored_glyph(
    stored: StoredFragmentGlyph,
    source_glyph_size: GlyphSize,
) -> Result<FragmentGlyph, JsonError> {
    let StoredFragmentGlyph {
        code,
        label,
        pixels,
    } = stored;
    let code = parse_code_in("fragment glyph code", &code)?;
    let context = format!("fragment glyph code {}", format_code(code));
    let bitmap: Bitmap =
        parse_rows(&pixels, source_glyph_size).map_err(|err| row_error_to_json(err, context))?;
    Ok(FragmentGlyph {
        code,
        label,
        bitmap,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use fontspace_model::GlyphSize;

    fn glyph(code: u32, label: &str, size: GlyphSize, on: &[(u16, u16)]) -> FragmentGlyph {
        let mut bitmap = Bitmap::new_blank(size);
        for &(x, y) in on {
            bitmap.set(x, y, true).unwrap();
        }
        FragmentGlyph {
            code,
            label: label.to_string(),
            bitmap,
        }
    }

    fn sample() -> FontSpaceFragment {
        let size = GlyphSize::new(5, 3);
        FontSpaceFragment::Glyphs(GlyphFragment {
            source_glyph_size: size,
            glyphs: vec![
                glyph(0x41, "A", size, &[(0, 0), (4, 2)]),
                glyph(0xE000, "SYMBOL", size, &[(2, 1)]),
            ],
        })
    }

    #[test]
    fn round_trips_through_json() {
        let fragment = sample();
        let json = save_fragment(&fragment);
        let back = load_fragment(&json).unwrap();
        assert_eq!(back, fragment);
    }

    #[test]
    fn re_serialization_is_stable() {
        // The determinism contract: save(load(save(f))) == save(f), byte-for-byte.
        let json = save_fragment(&sample());
        let reserialized = save_fragment(&load_fragment(&json).unwrap());
        assert_eq!(reserialized, json);
    }

    #[test]
    fn canonical_form_is_hex_codes_visual_rows_and_one_trailing_newline() {
        let json = save_fragment(&sample());
        assert!(json.ends_with("}\n"));
        assert!(!json.ends_with("}\n\n"));
        assert!(json.contains("\"fragment_version\": 1"));
        assert!(json.contains("\"kind\": \"glyphs\""));
        assert!(json.contains("\"code\": \"0x41\""));
        assert!(json.contains("\"code\": \"0xe000\"")); // lowercase hex
        assert!(json.contains("\"label\": \"SYMBOL\""));
        assert!(json.contains("\"#....\"")); // (0,0) on in a 5-wide row
    }

    #[test]
    fn rejects_an_unsupported_fragment_version() {
        let json = r#"{ "fragment_version": 99, "kind": "glyphs",
            "source_glyph_size": { "width": 5, "height": 3 }, "glyphs": [] }"#;
        let err = load_fragment(json).unwrap_err();
        assert!(matches!(
            err,
            JsonError::UnsupportedFragmentVersion {
                found: 99,
                supported: 1
            }
        ));
    }

    #[test]
    fn rejects_an_unknown_kind() {
        let json = r#"{ "fragment_version": 1, "kind": "pages",
            "source_glyph_size": { "width": 5, "height": 3 }, "glyphs": [] }"#;
        let err = load_fragment(json).unwrap_err();
        assert!(matches!(
            err,
            JsonError::UnknownFragmentKind { kind } if kind == "pages"
        ));
    }

    #[test]
    fn rejects_an_invalid_source_geometry() {
        let json = r#"{ "fragment_version": 1, "kind": "glyphs",
            "source_glyph_size": { "width": 0, "height": 3 }, "glyphs": [] }"#;
        let err = load_fragment(json).unwrap_err();
        assert!(matches!(
            err,
            JsonError::InvalidFragmentGeometry {
                width: 0,
                height: 3,
                ..
            }
        ));
    }

    #[test]
    fn rejects_a_glyph_whose_pixels_mismatch_the_source_geometry() {
        // source is 5×3 but the glyph supplies 2 rows → a RowCount error.
        let json = r#"{ "fragment_version": 1, "kind": "glyphs",
            "source_glyph_size": { "width": 5, "height": 3 },
            "glyphs": [ { "code": "0x41", "label": "A", "pixels": [".....", "....."] } ] }"#;
        let err = load_fragment(json).unwrap_err();
        assert!(matches!(
            err,
            JsonError::RowCount {
                expected: 3,
                found: 2,
                ..
            }
        ));
    }

    #[test]
    fn an_empty_glyph_fragment_round_trips() {
        let fragment = FontSpaceFragment::Glyphs(GlyphFragment {
            source_glyph_size: GlyphSize::new(8, 8),
            glyphs: Vec::new(),
        });
        let json = save_fragment(&fragment);
        assert_eq!(load_fragment(&json).unwrap(), fragment);
    }
}
