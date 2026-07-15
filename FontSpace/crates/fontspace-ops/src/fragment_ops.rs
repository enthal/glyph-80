//! Glyph fragment operations (spec/08). [`extract_glyphs`] copies a selection of
//! glyphs off a page into a serializable [`GlyphFragment`] (a pure query, no
//! mutation); [`paste_glyphs`] places a fragment into a destination page under an
//! explicit mapping and size policy, returning one invertible [`ChangeSet`].
//!
//! Two rules from spec/08 §8.3 (invariants in spec/17) shape this module: the paste
//! **never guesses** a destination code (the caller picks a [`GlyphMapping`]) and
//! **never silently resizes** (the default [`GlyphSizeConversion::RequireExact`]
//! rejects a geometry difference). Like every operation, a paste validates all
//! targets before mutating, so a failure leaves the document unchanged.

use fontspace_model::{Bitmap, FontSpace, FragmentGlyph, GlyphFragment, GlyphSetId, PageId};

use crate::apply_change_set;
use crate::change_set::{ChangeSet, GlyphChange, ObjectChange};
use crate::error::FontSpaceError;
use crate::selector::{GlyphSelector, resolve_glyph_codes};

/// How a pasted glyph's destination `code` is chosen (spec/08 §8.3). `BySlot`
/// (destination ordinal == source ordinal) lands with the ordinal-paste slice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GlyphMapping {
    /// Destination code equals the source glyph's `code` — the identity paste.
    ByCode,
    /// Destination codes count up from `start`, one per fragment glyph in order.
    SequentialFromCode(u32),
}

/// How a size difference between the fragment and the destination geometry is
/// resolved (spec/08 §8.3). The default `RequireExact` never resizes; `PlaceAt`,
/// `Center`, `Crop`, and `ScaleNearest` arrive with the size-conversion slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GlyphSizeConversion {
    /// Refuse any geometry difference — the safe default (spec/08 §8.3).
    #[default]
    RequireExact,
}

/// A request to copy glyphs off one page into a fragment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractGlyphs {
    pub glyph_set_id: GlyphSetId,
    pub page_id: PageId,
    pub glyphs: GlyphSelector,
}

/// A request to paste a fragment into a destination page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasteGlyphs {
    pub fragment: GlyphFragment,
    pub target_glyph_set_id: GlyphSetId,
    pub target_page_id: PageId,
    pub mapping: GlyphMapping,
    pub size_conversion: GlyphSizeConversion,
}

/// Copies the selected glyphs off `req.page_id` into a [`GlyphFragment`]. A pure
/// query: the document is not modified. Only codes with a **stored** glyph are
/// carried — an absent code is blank and holds nothing (spec/03 §3.6) — and each
/// glyph records its entry `label` so a later by-slot paste is possible (spec/08
/// §8.1). The fragment's `source_glyph_size` is the glyph set's geometry.
pub fn extract_glyphs(
    doc: &FontSpace,
    req: &ExtractGlyphs,
) -> Result<GlyphFragment, FontSpaceError> {
    let glyph_set = doc
        .glyph_set(req.glyph_set_id)
        .ok_or(FontSpaceError::GlyphSetNotFound(req.glyph_set_id))?;
    let character_set = doc.character_set(glyph_set.character_set_id).ok_or(
        FontSpaceError::CharacterSetNotFound {
            glyph_set: req.glyph_set_id,
            character_set: glyph_set.character_set_id,
        },
    )?;
    let page = glyph_set
        .page_of_id(req.page_id)
        .ok_or(FontSpaceError::PageIdNotFound {
            glyph_set: req.glyph_set_id,
            page: req.page_id,
        })?;
    let codes = resolve_glyph_codes(character_set, &req.glyphs)?;

    let glyphs = codes
        .iter()
        .filter_map(|&code| {
            page.glyph_of_code(code).map(|glyph| FragmentGlyph {
                code,
                label: character_set
                    .entry(code)
                    .map(|entry| entry.label.clone())
                    .unwrap_or_default(),
                bitmap: glyph.bitmap.clone(),
            })
        })
        .collect();

    Ok(GlyphFragment {
        source_glyph_size: glyph_set.glyph_size,
        glyphs,
    })
}

/// Pastes `req.fragment` into the destination page, returning an invertible change
/// set (empty if the paste changes nothing). Validates the geometry policy, resolves
/// and checks every destination code, and computes all changes **before** applying —
/// so a rejected paste (geometry mismatch, a destination code with no entry, or a
/// sequential-code overflow) leaves the document untouched (spec/07 §7.6, spec/08
/// §8.3).
pub fn paste_glyphs(doc: &mut FontSpace, req: &PasteGlyphs) -> Result<ChangeSet, FontSpaceError> {
    let object_changes = {
        let glyph_set = doc
            .glyph_set(req.target_glyph_set_id)
            .ok_or(FontSpaceError::GlyphSetNotFound(req.target_glyph_set_id))?;
        let character_set = doc.character_set(glyph_set.character_set_id).ok_or(
            FontSpaceError::CharacterSetNotFound {
                glyph_set: req.target_glyph_set_id,
                character_set: glyph_set.character_set_id,
            },
        )?;
        let page =
            glyph_set
                .page_of_id(req.target_page_id)
                .ok_or(FontSpaceError::PageIdNotFound {
                    glyph_set: req.target_glyph_set_id,
                    page: req.target_page_id,
                })?;

        let target_size = glyph_set.glyph_size;
        // Geometry policy: `RequireExact` refuses any size difference — no silent
        // resize (spec/08 §8.3). We check both the fragment's declared
        // `source_glyph_size` *and* every glyph's actual bitmap size, so a
        // malformed fragment (e.g. from a future clipboard/JSON loader, spec/08
        // §8.2) can never smuggle a wrong-size glyph past the geometry-agreement
        // invariant (spec/17). The per-glyph check moves into the size-conversion
        // arms once resizing conversions land.
        match req.size_conversion {
            GlyphSizeConversion::RequireExact => {
                if req.fragment.source_glyph_size != target_size {
                    return Err(FontSpaceError::GeometryMismatch {
                        glyph_set: req.target_glyph_set_id,
                        page: req.target_page_id,
                        source_size: req.fragment.source_glyph_size,
                        target_size,
                    });
                }
                for glyph in &req.fragment.glyphs {
                    if glyph.bitmap.size() != target_size {
                        return Err(FontSpaceError::GeometryMismatch {
                            glyph_set: req.target_glyph_set_id,
                            page: req.target_page_id,
                            source_size: glyph.bitmap.size(),
                            target_size,
                        });
                    }
                }
            }
        }

        let mut object_changes = Vec::new();
        for (index, glyph) in req.fragment.glyphs.iter().enumerate() {
            let code = match &req.mapping {
                GlyphMapping::ByCode => glyph.code,
                GlyphMapping::SequentialFromCode(start) => {
                    // Count up in the 32-bit code space; a run that would exceed it is
                    // rejected, never wrapped or saturated (spec/08 §8.3).
                    let code = *start as u64 + index as u64;
                    if code > u32::MAX as u64 {
                        return Err(FontSpaceError::SequentialCodeOverflow {
                            glyph_set: req.target_glyph_set_id,
                            page: req.target_page_id,
                            start: *start,
                            at_index: index,
                        });
                    }
                    code as u32
                }
            };
            // Pasting via an operation never creates a dangling glyph: the target
            // code must have an entry (spec/04 §4.4; same rule as `SetPixels`).
            if !character_set.contains_code(code) {
                return Err(FontSpaceError::CodeNotInCharacterSet {
                    character_set: glyph_set.character_set_id,
                    code,
                });
            }
            let before = page
                .glyph_of_code(code)
                .map(|existing| existing.bitmap.clone())
                .unwrap_or_else(|| Bitmap::new_blank(target_size));
            let after = glyph.bitmap.clone();
            if after != before {
                object_changes.push(ObjectChange::GlyphChanged(GlyphChange {
                    glyph_set_id: req.target_glyph_set_id,
                    page_id: req.target_page_id,
                    code,
                    before,
                    after,
                }));
            }
        }
        object_changes
    };

    let change_set = ChangeSet {
        object_changes,
        warnings: Vec::new(),
    };
    apply_change_set(doc, &change_set)?;
    Ok(change_set)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::undo as undo_change_set;
    use crate::{SetPixels, set_pixels};
    use fontspace_model::{
        CharacterEntry, CharacterSet, FontSpace, GlyphPage, GlyphSet, GlyphSize, SequentialIdGen,
    };
    use proptest::prelude::*;

    /// A two-glyph-set document sharing one character set is overkill here; instead
    /// build a single glyph set of `size` over a small ASCII-ish character set and
    /// draw a distinguishing pixel into a couple of codes on its one page.
    struct Fixture {
        doc: FontSpace,
        glyph_set_id: GlyphSetId,
        page_id: PageId,
    }

    fn fixture(size: GlyphSize, codes: &[u32]) -> Fixture {
        let mut ids = SequentialIdGen::new();
        let mut cs = CharacterSet::new(&mut ids, "set", "");
        cs.entries = codes
            .iter()
            .map(|&code| CharacterEntry {
                code,
                label: format!("code {code:#06x}"),
            })
            .collect();
        let cs_id = cs.id;
        let mut gs = GlyphSet::new(&mut ids, "gs", "", size, cs_id);
        let page = GlyphPage::new(&mut ids, "page", "");
        let page_id = page.id;
        let gs_id = gs.id;
        gs.pages.push(page);
        let mut doc = FontSpace::new(&mut ids, "doc", "");
        doc.character_sets.push(cs);
        doc.glyph_sets.push(gs);
        Fixture {
            doc,
            glyph_set_id: gs_id,
            page_id,
        }
    }

    /// Turn on pixel `(x, y)` of `code` on the fixture's page (materializing it).
    fn draw(fx: &mut Fixture, code: u32, x: u16, y: u16) {
        set_pixels(
            &mut fx.doc,
            &SetPixels {
                target: crate::GlyphRef {
                    glyph_set_id: fx.glyph_set_id,
                    page_id: fx.page_id,
                    code,
                },
                edits: vec![crate::PixelEdit { x, y, value: true }],
            },
        )
        .unwrap();
    }

    fn stored_pixel(fx: &Fixture, code: u32, x: u16, y: u16) -> bool {
        fx.doc
            .glyph_set(fx.glyph_set_id)
            .unwrap()
            .page_of_id(fx.page_id)
            .unwrap()
            .glyph_of_code(code)
            .map(|g| g.bitmap.get(x, y).unwrap())
            .unwrap_or(false)
    }

    #[test]
    fn extract_copies_stored_glyphs_with_labels_and_skips_absent_codes() {
        let size = GlyphSize::new(8, 8);
        let mut fx = fixture(size, &[0x41, 0x42, 0x43]);
        draw(&mut fx, 0x41, 1, 2);
        draw(&mut fx, 0x43, 3, 4);
        // 0x42 has no stored glyph → skipped.

        let fragment = extract_glyphs(
            &fx.doc,
            &ExtractGlyphs {
                glyph_set_id: fx.glyph_set_id,
                page_id: fx.page_id,
                glyphs: GlyphSelector::All,
            },
        )
        .unwrap();

        assert_eq!(fragment.source_glyph_size, size);
        let codes: Vec<u32> = fragment.glyphs.iter().map(|g| g.code).collect();
        assert_eq!(codes, vec![0x41, 0x43]); // 0x42 absent, entry order preserved
        assert_eq!(fragment.glyphs[0].label, "code 0x0041");
        assert!(fragment.glyphs[0].bitmap.get(1, 2).unwrap());
        assert!(fragment.glyphs[1].bitmap.get(3, 4).unwrap());
    }

    #[test]
    fn paste_by_code_reproduces_content_in_an_empty_destination() {
        // The spec/17 property, at a fixed instance: extract, then paste by code into
        // a compatible empty page, reproduces the glyphs.
        let size = GlyphSize::new(8, 8);
        let mut src = fixture(size, &[0x41, 0x42]);
        draw(&mut src, 0x41, 0, 0);
        draw(&mut src, 0x42, 7, 7);
        let fragment = extract_glyphs(
            &src.doc,
            &ExtractGlyphs {
                glyph_set_id: src.glyph_set_id,
                page_id: src.page_id,
                glyphs: GlyphSelector::All,
            },
        )
        .unwrap();

        let mut dst = fixture(size, &[0x41, 0x42]);
        let change = paste_glyphs(
            &mut dst.doc,
            &PasteGlyphs {
                fragment,
                target_glyph_set_id: dst.glyph_set_id,
                target_page_id: dst.page_id,
                mapping: GlyphMapping::ByCode,
                size_conversion: GlyphSizeConversion::RequireExact,
            },
        )
        .unwrap();

        assert!(!change.is_empty());
        assert!(stored_pixel(&dst, 0x41, 0, 0));
        assert!(stored_pixel(&dst, 0x42, 7, 7));
    }

    #[test]
    fn paste_is_invertible() {
        let size = GlyphSize::new(8, 8);
        let mut src = fixture(size, &[0x41]);
        draw(&mut src, 0x41, 2, 2);
        let fragment = extract_glyphs(
            &src.doc,
            &ExtractGlyphs {
                glyph_set_id: src.glyph_set_id,
                page_id: src.page_id,
                glyphs: GlyphSelector::Code(0x41),
            },
        )
        .unwrap();

        let mut dst = fixture(size, &[0x41]);
        assert!(!stored_pixel(&dst, 0x41, 2, 2));
        let change = paste_glyphs(
            &mut dst.doc,
            &PasteGlyphs {
                fragment,
                target_glyph_set_id: dst.glyph_set_id,
                target_page_id: dst.page_id,
                mapping: GlyphMapping::ByCode,
                size_conversion: GlyphSizeConversion::RequireExact,
            },
        )
        .unwrap();
        assert!(stored_pixel(&dst, 0x41, 2, 2));

        undo_change_set(&mut dst.doc, &change).unwrap();
        assert!(!stored_pixel(&dst, 0x41, 2, 2)); // back to the empty glyph
    }

    #[test]
    fn paste_require_exact_rejects_a_geometry_mismatch() {
        let mut src = fixture(GlyphSize::new(8, 8), &[0x41]);
        draw(&mut src, 0x41, 0, 0);
        let fragment = extract_glyphs(
            &src.doc,
            &ExtractGlyphs {
                glyph_set_id: src.glyph_set_id,
                page_id: src.page_id,
                glyphs: GlyphSelector::All,
            },
        )
        .unwrap();

        // Destination is 8×16 — different geometry.
        let mut dst = fixture(GlyphSize::new(8, 16), &[0x41]);
        let err = paste_glyphs(
            &mut dst.doc,
            &PasteGlyphs {
                fragment,
                target_glyph_set_id: dst.glyph_set_id,
                target_page_id: dst.page_id,
                mapping: GlyphMapping::ByCode,
                size_conversion: GlyphSizeConversion::RequireExact,
            },
        )
        .unwrap_err();
        assert_eq!(
            err,
            FontSpaceError::GeometryMismatch {
                glyph_set: dst.glyph_set_id,
                page: dst.page_id,
                source_size: GlyphSize::new(8, 8),
                target_size: GlyphSize::new(8, 16),
            }
        );
        // Atomic: nothing was written.
        assert!(!stored_pixel(&dst, 0x41, 0, 0));
    }

    #[test]
    fn paste_rejects_a_glyph_whose_bitmap_size_contradicts_the_fragment() {
        // A malformed fragment: it declares an 8×8 geometry (matching the target),
        // but one of its glyphs carries a differently-sized bitmap. RequireExact must
        // catch it per glyph rather than trust the declared size, so a future
        // clipboard/JSON loader cannot smuggle a wrong-size glyph in (spec/08 §8.2,
        // geometry-agreement invariant spec/17).
        let size = GlyphSize::new(8, 8);
        let mut fragment = {
            let mut src = fixture(size, &[0x41]);
            draw(&mut src, 0x41, 0, 0);
            extract_glyphs(
                &src.doc,
                &ExtractGlyphs {
                    glyph_set_id: src.glyph_set_id,
                    page_id: src.page_id,
                    glyphs: GlyphSelector::All,
                },
            )
            .unwrap()
        };
        // Corrupt the glyph's bitmap to a different size, leaving source_glyph_size 8×8.
        fragment.glyphs[0].bitmap = fontspace_model::Bitmap::new_blank(GlyphSize::new(8, 16));

        let mut dst = fixture(size, &[0x41]);
        let err = paste_glyphs(
            &mut dst.doc,
            &PasteGlyphs {
                fragment,
                target_glyph_set_id: dst.glyph_set_id,
                target_page_id: dst.page_id,
                mapping: GlyphMapping::ByCode,
                size_conversion: GlyphSizeConversion::RequireExact,
            },
        )
        .unwrap_err();
        assert_eq!(
            err,
            FontSpaceError::GeometryMismatch {
                glyph_set: dst.glyph_set_id,
                page: dst.page_id,
                source_size: GlyphSize::new(8, 16),
                target_size: size,
            }
        );
    }

    #[test]
    fn paste_rejects_a_destination_code_with_no_entry() {
        let size = GlyphSize::new(8, 8);
        let mut src = fixture(size, &[0x41]);
        draw(&mut src, 0x41, 0, 0);
        let fragment = extract_glyphs(
            &src.doc,
            &ExtractGlyphs {
                glyph_set_id: src.glyph_set_id,
                page_id: src.page_id,
                glyphs: GlyphSelector::All,
            },
        )
        .unwrap();

        // Destination charset lacks 0x41 (only 0x42), so a by-code paste is rejected
        // rather than creating a dangling glyph.
        let mut dst = fixture(size, &[0x42]);
        let err = paste_glyphs(
            &mut dst.doc,
            &PasteGlyphs {
                fragment,
                target_glyph_set_id: dst.glyph_set_id,
                target_page_id: dst.page_id,
                mapping: GlyphMapping::ByCode,
                size_conversion: GlyphSizeConversion::RequireExact,
            },
        )
        .unwrap_err();
        assert!(matches!(
            err,
            FontSpaceError::CodeNotInCharacterSet { code: 0x41, .. }
        ));
    }

    #[test]
    fn paste_sequential_from_code_maps_to_consecutive_codes() {
        let size = GlyphSize::new(8, 8);
        let mut src = fixture(size, &[0x41, 0x42]);
        draw(&mut src, 0x41, 0, 0);
        draw(&mut src, 0x42, 1, 1);
        let fragment = extract_glyphs(
            &src.doc,
            &ExtractGlyphs {
                glyph_set_id: src.glyph_set_id,
                page_id: src.page_id,
                glyphs: GlyphSelector::All,
            },
        )
        .unwrap();

        // Destination codes 0x50, 0x51 receive the two glyphs in fragment order.
        let mut dst = fixture(size, &[0x50, 0x51]);
        paste_glyphs(
            &mut dst.doc,
            &PasteGlyphs {
                fragment,
                target_glyph_set_id: dst.glyph_set_id,
                target_page_id: dst.page_id,
                mapping: GlyphMapping::SequentialFromCode(0x50),
                size_conversion: GlyphSizeConversion::RequireExact,
            },
        )
        .unwrap();
        assert!(stored_pixel(&dst, 0x50, 0, 0)); // first fragment glyph
        assert!(stored_pixel(&dst, 0x51, 1, 1)); // second fragment glyph
    }

    #[test]
    fn paste_of_identical_content_is_a_no_op() {
        let size = GlyphSize::new(8, 8);
        let mut src = fixture(size, &[0x41]);
        draw(&mut src, 0x41, 4, 4);
        let fragment = extract_glyphs(
            &src.doc,
            &ExtractGlyphs {
                glyph_set_id: src.glyph_set_id,
                page_id: src.page_id,
                glyphs: GlyphSelector::All,
            },
        )
        .unwrap();

        // Destination already has the identical glyph → paste records no change.
        let mut dst = fixture(size, &[0x41]);
        draw(&mut dst, 0x41, 4, 4);
        let change = paste_glyphs(
            &mut dst.doc,
            &PasteGlyphs {
                fragment,
                target_glyph_set_id: dst.glyph_set_id,
                target_page_id: dst.page_id,
                mapping: GlyphMapping::ByCode,
                size_conversion: GlyphSizeConversion::RequireExact,
            },
        )
        .unwrap();
        assert!(change.is_empty());
    }

    #[test]
    fn sequential_paste_overflowing_the_code_space_is_rejected() {
        let size = GlyphSize::new(8, 8);
        // A two-glyph fragment.
        let mut src = fixture(size, &[0x41, 0x42]);
        draw(&mut src, 0x41, 0, 0);
        draw(&mut src, 0x42, 0, 0);
        let fragment = extract_glyphs(
            &src.doc,
            &ExtractGlyphs {
                glyph_set_id: src.glyph_set_id,
                page_id: src.page_id,
                glyphs: GlyphSelector::All,
            },
        )
        .unwrap();

        // The destination has the u32::MAX entry so the *first* glyph maps cleanly;
        // the second (start + 1) overflows the code space and is rejected.
        let mut dst = fixture(size, &[u32::MAX]);
        let err = paste_glyphs(
            &mut dst.doc,
            &PasteGlyphs {
                fragment,
                target_glyph_set_id: dst.glyph_set_id,
                target_page_id: dst.page_id,
                mapping: GlyphMapping::SequentialFromCode(u32::MAX),
                size_conversion: GlyphSizeConversion::RequireExact,
            },
        )
        .unwrap_err();
        assert!(matches!(
            err,
            FontSpaceError::SequentialCodeOverflow {
                start: u32::MAX,
                at_index: 1,
                ..
            }
        ));
    }

    /// The stored (non-blank) glyphs of the fixture's page, as `(code, bitmap)` in
    /// entry order — the canonical content to compare a round-trip against.
    fn stored_glyphs(fx: &Fixture) -> Vec<(u32, fontspace_model::Bitmap)> {
        fx.doc
            .glyph_set(fx.glyph_set_id)
            .unwrap()
            .page_of_id(fx.page_id)
            .unwrap()
            .glyphs
            .iter()
            .filter(|glyph| !glyph.bitmap.is_blank())
            .map(|glyph| (glyph.code, glyph.bitmap.clone()))
            .collect()
    }

    proptest! {
        /// The spec/17 property: extract, then paste by code into a compatible empty
        /// destination, reproduces the content exactly (spec/08 §8.4, CLAUDE.md
        /// property-test list). `pixels_per_code` seeds each of the three codes with
        /// an arbitrary on-pixel set (an empty set leaves that glyph blank/absent).
        #[test]
        fn extract_then_paste_reproduces_content(
            pixels_per_code in prop::collection::vec(
                prop::collection::hash_set((0u16..8, 0u16..8), 0..16),
                3,
            ),
        ) {
            let size = GlyphSize::new(8, 8);
            let codes = [0x41u32, 0x42, 0x43];

            let mut src = fixture(size, &codes);
            for (code, pixels) in codes.iter().zip(&pixels_per_code) {
                for &(x, y) in pixels {
                    draw(&mut src, *code, x, y);
                }
            }
            let fragment = extract_glyphs(
                &src.doc,
                &ExtractGlyphs {
                    glyph_set_id: src.glyph_set_id,
                    page_id: src.page_id,
                    glyphs: GlyphSelector::All,
                },
            )
            .unwrap();

            let mut dst = fixture(size, &codes);
            paste_glyphs(
                &mut dst.doc,
                &PasteGlyphs {
                    fragment,
                    target_glyph_set_id: dst.glyph_set_id,
                    target_page_id: dst.page_id,
                    mapping: GlyphMapping::ByCode,
                    size_conversion: GlyphSizeConversion::RequireExact,
                },
            )
            .unwrap();

            prop_assert_eq!(stored_glyphs(&dst), stored_glyphs(&src));
        }
    }
}
