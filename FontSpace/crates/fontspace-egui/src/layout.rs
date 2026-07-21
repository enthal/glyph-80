//! The set of tiled views and the opinionated default layout (spec/12 §12.1).
//!
//! Layout construction is a **pure function** of nothing but the [`Pane`] set, so it
//! is unit-tested here without an `egui` context or event loop (spec/15, CLAUDE.md
//! "if logic can be tested without a UI, it must not live inside a UI function").

use egui_tiles::{Tile, Tree};

/// One tiled view. Every major view in spec/12 §12.1 is a pane kind; the ones that
/// only make sense with multiple documents (Character Across Pages/Files) or later
/// milestones (the Export **Preview** / raw-memory view) arrive with those features.
/// This shell covers the single-document editor set plus the Export Configuration
/// editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Pane {
    GlyphEditor,
    CharacterSet,
    PageOverview,
    TextPreview,
    ExportConfiguration,
    DocumentBrowser,
    Inspector,
}

impl Pane {
    /// The panes the default layout is built from, in a stable order.
    pub const ALL: [Pane; 7] = [
        Pane::DocumentBrowser,
        Pane::GlyphEditor,
        Pane::CharacterSet,
        Pane::PageOverview,
        Pane::TextPreview,
        Pane::ExportConfiguration,
        Pane::Inspector,
    ];

    /// The tab/title text for this pane.
    pub fn title(self) -> &'static str {
        match self {
            Pane::GlyphEditor => "Glyph Editor",
            Pane::CharacterSet => "Character Set",
            Pane::PageOverview => "Page Overview",
            Pane::TextPreview => "Text Preview",
            Pane::ExportConfiguration => "Export Configuration",
            Pane::DocumentBrowser => "Documents",
            Pane::Inspector => "Inspector",
        }
    }
}

/// Builds the opinionated default layout (spec/12 §12.1): a document browser on the
/// left, the glyph editor dominant in the center above a tab strip of Character Set /
/// Page Overview / Text Preview / Export Configuration, and the inspector on the right.
///
/// ```text
/// +-----------+-----------------------------+-----------+
/// | Documents | Glyph Editor                | Inspector |
/// |           +-----------------------------+           |
/// |           | Char / Pages / Preview / Ex |           |
/// +-----------+-----------------------------+-----------+
/// ```
pub fn default_tree() -> Tree<Pane> {
    let mut tiles = egui_tiles::Tiles::default();

    let editor = tiles.insert_pane(Pane::GlyphEditor);
    let character_set = tiles.insert_pane(Pane::CharacterSet);
    let page_overview = tiles.insert_pane(Pane::PageOverview);
    let text_preview = tiles.insert_pane(Pane::TextPreview);
    let export_config = tiles.insert_pane(Pane::ExportConfiguration);
    let tabs = tiles.insert_tab_tile(vec![
        character_set,
        page_overview,
        text_preview,
        export_config,
    ]);
    let center = tiles.insert_vertical_tile(vec![editor, tabs]);

    let browser = tiles.insert_pane(Pane::DocumentBrowser);
    let inspector = tiles.insert_pane(Pane::Inspector);
    let root = tiles.insert_horizontal_tile(vec![browser, center, inspector]);

    Tree::new("fontspace_tiles", root, tiles)
}

/// The panes currently present in a tree, in the tree's internal storage order.
/// Used by tests and by "focus/reveal" commands; not a rendering path.
pub fn panes_in(tree: &Tree<Pane>) -> Vec<Pane> {
    tree.tiles
        .tiles()
        .filter_map(|tile| match tile {
            Tile::Pane(pane) => Some(*pane),
            Tile::Container(_) => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn default_layout_contains_every_pane_exactly_once() {
        let tree = default_tree();
        let mut panes = panes_in(&tree);
        panes.sort_by_key(|p| format!("{p:?}"));

        let mut expected = Pane::ALL.to_vec();
        expected.sort_by_key(|p| format!("{p:?}"));

        assert_eq!(
            panes, expected,
            "default layout must include all panes once"
        );
    }

    #[test]
    fn default_layout_has_no_duplicate_panes() {
        let tree = default_tree();
        let panes = panes_in(&tree);
        let unique: HashSet<_> = panes.iter().copied().collect();
        assert_eq!(panes.len(), unique.len(), "no pane may appear twice");
    }

    #[test]
    fn default_tree_is_deterministic() {
        // Two builds must agree on the pane set (the reset command is just a
        // rebuild). Tile storage iterates in hash order, so compare as sets. The
        // reset command's actual round-trip is covered in `app.rs`.
        let a: HashSet<_> = panes_in(&default_tree()).into_iter().collect();
        let b: HashSet<_> = panes_in(&default_tree()).into_iter().collect();
        assert_eq!(a, b);
    }

    #[test]
    fn every_pane_has_a_nonempty_title() {
        for pane in Pane::ALL {
            assert!(!pane.title().is_empty(), "{pane:?} needs a title");
        }
    }
}
